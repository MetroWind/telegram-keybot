use std::str;
use std::fs;
use std::io::prelude::*;
use std::time;
use std::path::PathBuf;

use rand::prelude::*;
use futures::StreamExt;
use log::{info,debug};
use log::error as log_error;
use toml;
use tokio;
use serde_json;
use serde::{Serialize, Deserialize};
use telegram_bot as bot;
use telegram_bot::types::{Message, MessageKind};
use telegram_bot::types::requests::{SendMessage, SendPhoto};
use reqwest;
use reqwest::header::CONTENT_LENGTH;
use tempfile;
use chrono;

use crate::error::Error;
use crate::utils;
use crate::reddit;

static TG_IMG_SIZE_LIMIT: u32 = 4096;
static TG_IMG_FILE_SIZE_LIMIT: u64 = 5 * 1024 * 1024;
static IMG_RESIZE_TARGET: u32 = 1024;
static IMG_RESIZE_QUALITY: u32 = 92;

#[derive(Serialize, Deserialize, Clone)]
pub struct ConfigParamsGeneral
{
    pub do_welcome: bool,
    pub welcome: String,
    pub token: String,
    pub username: String,
    pub group_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ConfigParamsReddit
{
    pub client_id: String,
    pub client_secret: String,
    pub daily_pic_caption: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ConfigParams
{
    pub general: ConfigParamsGeneral,
    pub reddit: ConfigParamsReddit,
}

impl ConfigParams
{
    pub fn fromFile(filename: &str) -> Result<Self, Error>
    {
        let mut file = fs::File::open(filename).map_err(
            |_| {error!(RuntimeError, format!("Failed to open file {}",
                                              filename))})?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).map_err(
            |_| {error!(RuntimeError,
                        format!("Failed to read file {}", filename))})?;

        toml::from_str(&contents).map_err(
            |e| {error!(RuntimeError,
                        format!("Failed to parse file {}: {}", filename, e))})
    }
}

fn getTempFile(suffix_maybe: Option<&str>) -> Result<PathBuf, Error>
{
    let named = if let Some(suffix) = suffix_maybe {
        tempfile::Builder::new().prefix("keybot-").suffix(suffix).tempfile()
    }
    else
    {
        tempfile::Builder::new().prefix("keybot-").tempfile()
    }.map_err(|_| error!(RuntimeError, "Failed to create temp file"))?;
    let (_, path) = named.keep().map_err(
        |_| error!(RuntimeError, "Failed to keep temp file"))?;
    Ok(path)
}

fn parseMagickSizeOutput(output: &str) -> Result<(u32, u32), Error>
{
    let mut parts = output.split('x');
    let err = error!(RuntimeError, format!("Invalid magick size string: {}", output));
    let width: u32 = parts.next().ok_or(err.clone())?.parse()
        .map_err(|_| err.clone())?;
    let height: u32 = parts.next().ok_or(err.clone())?.parse()
        .map_err(|_| err.clone())?;
    Ok((width, height))
}


fn getImageSize(uri: &str) -> Result<(u32, u32), Error>
{
    let (output_raw, _) = utils::runWithOutput(
        &["magick", "identify", "-ping", "-format", "%wx%h", uri])?;
    let output = str::from_utf8(&output_raw).map_err(
        |_| error!(RuntimeError, "Invalid magick output"))?;
    debug!("Magick output {}", output);
    parseMagickSizeOutput(output)
}

#[derive(Serialize, Deserialize)]
pub struct RuntimeInfo
{
    chat_id: Vec<i64>,
    last_msg_id: Option<i64>,
    wa_count: u32,
}

impl RuntimeInfo
{
    const FILE: &'static str = "runtime-info.json";

    pub fn new() -> Self
    {
        Self {
            chat_id: Vec::new(),
            last_msg_id: None,
            wa_count: 0,
        }
    }

    pub fn exist() -> bool
    {
        std::path::Path::new(Self::FILE).exists()
    }

    pub fn load() -> Result<Self, Error>
    {
        let mut file = fs::File::open(Self::FILE).map_err(
            |_| {error!(RuntimeError, format!("Failed to open file {}",
                                              Self::FILE))})?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).map_err(
            |_| {error!(RuntimeError,
                        format!("Failed to read file {}", Self::FILE))})?;

        serde_json::from_str(&contents).map_err(
            |_| {error!(RuntimeError,
                        format!("Failed to parse file {}", Self::FILE))})
    }

    pub fn save(&self) -> Result<(), Error>
    {
        let mut file = fs::File::create(Self::FILE).map_err(
            |_| {error!(RuntimeError, format!("Failed to open file
                        {}", Self::FILE))})?;
        file.write_all(serde_json::to_string(self).map_err(
            |_| {error!(RuntimeError, "Failed to generate runtime info")})?
                       .as_bytes()).map_err(
            |_| {error!(RuntimeError,
                        format!("Failed to write file {}", Self::FILE))})
    }
}

async fn replyWithDelay(api: &bot::Api, msg: String,
                        reply_to_id: bot::MessageId, chat_id: bot::ChatId,
                        delay_sec: f64) -> Result<Message, Error>
{
    tokio::time::delay_for(
        time::Duration::from_millis((delay_sec * 1000.0) as u64)).await;
    let post = api.send(SendMessage::new(chat_id, msg).reply_to(reply_to_id))
        .await.map_err(|_| error!(RuntimeError, "Failed to reply with delay"))?;
    if let bot::types::MessageOrChannelPost::Message(msg) = post
    {
        Ok(msg)
    }
    else
    {
        Err(error!(RuntimeError, "Shit happened."))
    }

    // thread::spawn(move || {
    //     thread::sleep(
    //     let api = bot::Api::new(bot_token);
    //     if let Ok(mut env) = tokio::runtime::Runtime::new()
    //     {
    //         if let Err(e) = env.block_on(api.send(
    //             SendMessage::new(chat_id, msg).reply_to(reply_to_id)))
    //         {
    //             log_error!("{}", e);
    //         }
    //     }
    //     else
    //     {
    //         log_error!("Failed to create Tokio runtime.");
    //     }
    // });
}

fn getUserFullname(u: &bot::User) -> String
{
    if let Some(last) = &u.last_name
    {
        format!("{} {}", u.first_name, last)
    }
    else
    {
        u.first_name.clone()
    }
}

fn getUsername(u: &bot::User) -> String
{
    if let Some(nick) = &u.username
    {
        format!("@{}", nick)
    }
    else
    {
        getUserFullname(u)
    }
}

async fn welcome(api: &bot::Api, config: &ConfigParams,
                 new_users: &Vec<bot::User>, chat: &bot::Channel)
                 -> Result<(), Error>
{
    if config.general.welcome.is_empty()
    {
        return Err(error!(RuntimeError, "No welcome message set"));
    }

    let mut err = false;
    for user in new_users
    {
        let name = getUsername(user);
        info!("{} joined chat {} ({}).", name, chat.id, chat.title);
        let msg_tplt = utils::SimpleTemplate::new(&config.general.welcome);
        let msg = msg_tplt
            .apply("user", format!("tg://user?id={}", getUserFullname(user)))
            .result();
        if api.send(SendMessage::new(chat.id, msg)).await.is_err()
        {
            err = true;
        }
    }
    if err
    {
        Err(error!(RuntimeError, "Failed to welcome"))
    }
    else
    {
        Ok(())
    }
}

async fn onNewChatMembers(api: &bot::Api, config: &ConfigParams,
                          new_users: &Vec<bot::User>, chat: &bot::Channel)
                          -> Result<(), Error>
{
    debug!("New members in chat {} ({}).", chat.id, chat.title);
    // Is the bot one of the new members?
    if new_users.iter().any(|u|
        match &u.username
        {
            Some(nick) => nick == &config.general.username,
            None => false,
        })
    {
        let mut info = RuntimeInfo::load()?;
        if !info.chat_id.contains(&(i64::from(chat.id)))
        {
            info.chat_id.push(i64::from(chat.id));
            if let Err(e) = info.save()
            {
                log_error!("{}", e);
            }
        }
    }
    else if config.general.do_welcome
    {
        welcome(api, config, new_users, chat).await?;
    }
    Ok(())
}

async fn onWaReply(api: &bot::Api, config: &ConfigParams, msg: &Message)
                   -> Result<(), Error>
{
    let chat_id = msg.chat.id();
    // Sliently ignore if the reply is not sent in the correct chat.
    if let Some(correct_chat) = config.general.group_id
    {
        if chat_id != bot::types::ChatId::new(correct_chat)
        {
            return Ok(());
        }
    }
    else
    {
        return Ok(());
    }

    let mut info = RuntimeInfo::load()?;
    let reddit_msg_id = if let Some(id) = info.last_msg_id
    {
        id
    }
    else
    {
        return Ok(());
    };

    debug!("It's a wa. Wa count was {}", info.wa_count);
    if info.wa_count == 2
    {
        let delay: f64 = thread_rng().gen_range(10.0, 600.0);
        info.wa_count = 0;
        info.last_msg_id = None;
        info.save()?;
        replyWithDelay(api, "哇！".to_string(),
                       bot::types::MessageId::new(reddit_msg_id), chat_id,
                       delay).await?;
    }
    else
    {
        info.wa_count += 1;
        info.save()?;
    }
    Ok(())
}

async fn onTextReplyToMsg(api: &bot::Api, config: &ConfigParams, msg: &Message,
                          reply_to: &Message) -> Result<(), Error>
{
    debug!("Reply to {} receivd.", reply_to.id);
    let info = RuntimeInfo::load()?;
    if let Some(daily_id) = info.last_msg_id
    {
        if daily_id != i64::from(reply_to.id)
        {
            return Ok(());
        }

        if let MessageKind::Text{ref data, ..} = msg.kind
        {
            if data.starts_with("哇")
            {
                onWaReply(api, config, msg).await?;
            }
        }
    }
    Ok(())
}


async fn onTextReply(api: &bot::Api, config: &ConfigParams, msg: &Message,
                     reply_to: &bot::types::MessageOrChannelPost)
                     -> Result<(), Error>
{
    match reply_to
    {
        telegram_bot::types::MessageOrChannelPost::Message(parent) =>
        {
            onTextReplyToMsg(api, config, msg, &parent).await?;
        },
        telegram_bot::types::MessageOrChannelPost::ChannelPost(_) => (),
    }
    Ok(())
}

fn downloadFile(uri: &str, to_file: &str) -> Result<(), Error>
{
    utils::run(&["curl", "--silent", "-o", to_file, uri])
}

/// Information of a file on the web.
struct UriFileInfo
{
    /// The file size
    size: u64,
    /// The downloaded file name
    filename: Option<String>,
}

/// Get the file size pointed by `uri`. This may download the file. If
/// it does, return (size, downloaded filename), otherwise
/// return (size, None).
async fn getUriFileSize(uri: &str) -> Result<UriFileInfo, Error>
{
    // Here I cannot use reqwest::blocking, because it would mess up
    // the scheduling of tokio threads in the bot.
    let res = reqwest::Client::new().head(uri).send().await.map_err(
        |_| error!(RuntimeError,
                   format!("Failed to send head request for {}", uri)))?;
    if let Err(_) = res.error_for_status_ref()
    {
        return Err(error!(RuntimeError, format!(
            "Failed to get head for {}", uri)));
    }

    let headers = res.headers();
    if headers.contains_key(CONTENT_LENGTH)
    {
        let size_str = headers[CONTENT_LENGTH].to_str().map_err(
            |_| error!(RuntimeError, "Failed to get size from header"))?;
        debug!("Got file size from header: {}", size_str);
        Ok(UriFileInfo{
            size: size_str.parse().map_err(
                |_| error!(RuntimeError, format!("Invalid size from header: {}", size_str)))?,
            filename: None,
        })
    }
    else
    {
        let temp_path = getTempFile(None)?;
        let temp = temp_path.to_str().ok_or(
            error!(RuntimeError, "Failed to encode temp file path"))?;
        info!("Downloading {} into {}...", uri, temp);
        downloadFile(uri, temp)?;
        Ok(UriFileInfo {
            size: fs::metadata(temp).map_err(
                |_| error!(RuntimeError, "Failed to get size of temp file"))?
                .len(),
            filename: Some(temp.to_owned()),
        })
    }
}

async fn sendPhoto(api: &bot::Api, uri: &str, caption: &str, chat_id: i64)
                       -> Result<Message, Error>
{
    debug!("Sending photo at {}...", uri);
    let size = getImageSize(uri)?;
    debug!("Image size is {}x{}.", size.0, size.1);
    let file_info = getUriFileSize(uri).await?;
    // debug!("File size for {} is {}.", file_info.filename, file_info.size);
    if size.0 < TG_IMG_SIZE_LIMIT && size.1 < TG_IMG_SIZE_LIMIT
        && file_info.size < TG_IMG_FILE_SIZE_LIMIT
    {
        api.send(SendPhoto::new(bot::types::ChatId::new(chat_id),
                                bot::types::InputFileRef::new(uri))
                 .caption(caption)).await.map_err(
            |_| error!(RuntimeError, "Failed to send photo"))
    }
    else
    {
        info!("Processing large image file...");
        // Download image.
        let img_orig: String = if let Some(f) = &file_info.filename
        {
            f.to_string()
        }
        else
        {
            let f = getTempFile(None)?.to_str().ok_or(
                error!(RuntimeError, "Failed to encode temp file path"))?
                .to_string();
            downloadFile(uri, &f)?;
            f
        };
        // Resize
        info!("Resizing image to {}...", IMG_RESIZE_TARGET);
        // Due to the limitation of the VPS, this is surprisingly easy to
        // fail.
        let img_resized = getTempFile(Some(".jpg"))?.to_str().ok_or(
            error!(RuntimeError, "Failed to encode temp file path"))?
            .to_string();
        let img_orig_ref = &img_orig;
        if let Err(e) = utils::run(
            &["magick", "convert", img_orig_ref,
              "-limit", "memory", "100MiB",
              // "-limit", "map", "200MiB",
              "-resize", &format!("{s}x{s}", s=IMG_RESIZE_TARGET),
              "-quality", &IMG_RESIZE_QUALITY.to_string(),
              &img_resized])
        {
            fs::remove_file(img_orig_ref).map_err(
                |_| error!(RuntimeError,
                           format!("Failed to remove temp file: {}", img_orig_ref)))?;
            return Err(e);
        }
        fs::remove_file(img_orig_ref).map_err(
            |_| error!(RuntimeError,
                       format!("Failed to remove temp file: {}", img_orig_ref)))?;
        api.send(SendPhoto::new(
            bot::types::ChatId::new(chat_id),
            bot::types::InputFileUpload::with_path(img_resized))
                 .caption(caption)).await.map_err(
            |_| error!(RuntimeError, "Failed to send photo"))
    }
}

async fn trySendFirstPhotoFromPosts(api: &bot::Api, chat_id: i64,
                              reddit_posts: &Vec<&reddit::Post>,
                              caption_tplt: &str) -> Result<Message, Error>
{
    for best_post in reddit_posts
    {
        info!("Best reddit post today is {}, with image at {}.",
              best_post.shortUrl(), &best_post.link);

        if let Ok(msg) =
            sendPhoto(api, &best_post.link,
                      &utils::SimpleTemplate::new(caption_tplt)
                      .apply("url", best_post.shortUrl()).result(),
                      chat_id).await
        {
            return Ok(msg);
        }
        else
        {
            log_error!("Failed to send post {} as the best daily post.",
                       best_post.url);
        }
    }
    Err(error!(RuntimeError, "Failed to send best post"))
}

async fn getRedditPostsToday(config: &ConfigParams)
                             -> Result<Vec<reddit::Post>, Error>
{
    // API rule says must authenticate.
    // https://github.com/reddit-archive/reddit/wiki/API
    debug!("Authenticating on Reddit...");
    let redditor = reddit::RedditQuerier::fromUserlessAuthentication(
        &config.reddit.client_id, &config.reddit.client_secret).await?;

    let sub = reddit::Subreddit::new("MechanicalKeyboards");
    debug!("Getting posts...");
    let mut posts = sub.list(&redditor, reddit::PostSorting::new, None, None)
        .await?;
    let now = chrono::offset::Utc::now();
    let yesterday = now - chrono::Duration::days(1);

    {
        while posts.last().unwrap().time_create > yesterday
        {
            let last_fullname: String = posts.last().unwrap().fullName()
                .to_owned();
            posts.append(&mut sub.list(&redditor, reddit::PostSorting::new, None,
                                       Some(&last_fullname)).await?);
        }
    }

    for i in (0..=posts.len()-1).rev()
    {
        if posts[i].time_create > yesterday
        {
            posts.truncate(i + 1);
            break;
        }
    }

    debug!("Logging out of Reddit...");
    if let Err(e) = redditor.logout().await
    {
        log_error!("{}", e);
    }
    Ok(posts)
}

pub async fn sendBestRedditToday(api: &bot::Api, config: &ConfigParams)
                                 -> Result<Message, Error>
{
    if config.general.group_id.is_none()
    {
        return Err(error!(RuntimeError, "No group ID specified"));
    }

    let chat_id = config.general.group_id.unwrap();
    let posts = getRedditPostsToday(config).await?;
    let mut best_posts: Vec<&reddit::Post> = posts.iter().filter(|p| {
        if !p.isLink()
        {
            return false;
        }

        let link = p.link.to_lowercase();
        link.ends_with(".jpg") || link.ends_with(".jpeg") ||
            link.ends_with(".png")
    }).collect();
    best_posts.sort_by_key(|p| p.score);
    best_posts.reverse();

    let msg = trySendFirstPhotoFromPosts(
        api, chat_id, &best_posts, &config.reddit.daily_pic_caption).await?;

    let mut info = RuntimeInfo::load()?;
    info.last_msg_id = Some(i64::from(msg.id));
    info.save()?;
    Ok(msg)
}

async fn onMessage(api: &bot::Api, config: &ConfigParams, msg: Message)
                   -> Result<(), Error>
{
    match msg.kind
    {
        // MessageKind::Text { ref data, .. } =>
        MessageKind::Text {..} =>
        {
            if let Some(reply_to_box) = &msg.reply_to_message
            {
                onTextReply(api, config, &msg, reply_to_box.as_ref()).await?;
            }
        },
        _ => ()
    }
    Ok(())
}

async fn onChannelPost(api: &bot::Api, config: &ConfigParams,
                       post: bot::types::ChannelPost) -> Result<(), Error>
{
    match post.kind
    {
        MessageKind::NewChatMembers{ref data} =>
        {
            onNewChatMembers(api, &config, data, &post.chat).await?;
        },
        _ => ()
    }
    Ok(())
}

pub async fn startBot(config: &ConfigParams)
{
    let api = bot::Api::new(&config.general.token);
    let mut stream = api.stream();
    info!("Entering update loop...");
    while let Some(update) = stream.next().await
    {
        let update = match update
        {
            Err(e) => {log_error!("{}", e); continue;},
            Ok(u) => u,
        };

        let api = api.clone();
        // TODO: maybe use an arc instead of cloning?
        let config = config.clone();
        tokio::spawn(async move {
            match update.kind
            {
                bot::types::UpdateKind::Message(message) =>
                {
                    if let Err(e) = onMessage(&api, &config, message).await
                    {
                        log_error!("{}", e);
                    }
                },
                bot::types::UpdateKind::ChannelPost(post) =>
                {
                    if let Err(e) = onChannelPost(&api, &config, post).await
                    {
                        log_error!("{}", e);
                    }
                },
                _ => (),
            }
        });
    }
}
