use std::str;
use std::fs;
use std::io::prelude::*;

use rand::prelude::*;
use futures::StreamExt;
use log::{info,debug};
use log::error as log_error;
use tokio;
use serde_json;
use serde::{Serialize, Deserialize};
use telegram_bot as bot;
use telegram_bot::types::{Message, MessageKind};
use telegram_bot::types::requests::SendMessage;
use chrono::prelude::*;
use chrono;

use crate::error::Error;
use crate::utils;
use crate::reddit;
use crate::bot_config;
use crate::telegram;
use crate::chat_db;

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

async fn welcome(api: &bot::Api, config: &bot_config::ConfigParams,
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
        let name = telegram::getUsername(user);
        info!("{} joined chat {} ({}).", name, chat.id, chat.title);
        let msg_tplt = utils::SimpleTemplate::new(&config.general.welcome);
        let msg = msg_tplt
            .apply("user", format!("tg://user?id={}",
                                   telegram::getUserFullname(user)))
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

async fn onNewChatMembers(api: &bot::Api, config: &bot_config::ConfigParams,
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

async fn onWaReply(api: &bot::Api, config: &bot_config::ConfigParams, msg: &Message)
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

    // Add to chat DB.
    let waable_id = telegram::getParentMsgId(msg)
        .ok_or_else(|| error!(RuntimeError, "Wa is not a reply"))?;

    let wa_count = chat_db::addWa(chat_db::WaEntry {
        wa_to: i64::from(waable_id),
        id: i64::from(msg.id),
        waer: i64::from(msg.from.id),
        waer_name: telegram::getUsername(&msg.from),
        time: chrono::Utc.timestamp(msg.date, 0),
    })?;

    debug!("It's a wa. Wa count is {}", wa_count);
    if wa_count == 3
    {
        let delay: f64 = thread_rng().gen_range(10.0, 600.0);
        telegram::replyWithDelay(
            api, "哇！".to_string(), waable_id, chat_id, delay).await?;
    }
    Ok(())
}

async fn onTextReplyToMsg(api: &bot::Api, config: &bot_config::ConfigParams,
                          msg: &Message, reply_to: &Message) -> Result<(), Error>
{
    debug!("Reply to {} receivd.", reply_to.id);
    if let MessageKind::Text{ref data, ..} = msg.kind
    {
        if data.starts_with("哇")
        {
            onWaReply(api, config, msg).await?;
        }
    }
    Ok(())
}


async fn onTextReply(api: &bot::Api, config: &bot_config::ConfigParams,
                     msg: &Message, reply_to: &bot::types::MessageOrChannelPost)
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

async fn trySendFirstPhotoFromPosts(api: &bot::Api, chat_id: i64,
                              reddit_posts: &Vec<&reddit::Post>,
                              caption_tplt: &str) -> Result<Message, Error>
{
    for best_post in reddit_posts
    {
        info!("Best reddit post today is {}, with image at {}.",
              best_post.shortUrl(), &best_post.link);

        if let Ok(msg) =
            telegram::sendPhoto(
                api, &best_post.link, &utils::SimpleTemplate::new(caption_tplt)
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

async fn getRedditPostsToday(config: &bot_config::ConfigParams)
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

pub async fn sendBestRedditToday(api: &bot::Api, config: &bot_config::ConfigParams)
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
    info.wa_count = 0;
    info.save()?;
    Ok(msg)
}

async fn onMessage(api: &bot::Api, config: &bot_config::ConfigParams, msg: Message)
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

async fn onChannelPost(api: &bot::Api, config: &bot_config::ConfigParams,
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

pub async fn startBot(config: &bot_config::ConfigParams)
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

pub async fn sendBestWaer(
    api: &bot::Api, chat_id: i64, time_period: chrono::Duration, msg_tplt: &str)
    -> Result<(), Error>
{
    let (waer, count) = chat_db::bestWaer(time_period)?;
    info!("Best waer the last {}, with {} was.", time_period, count);

    api.send(SendMessage::new(
        bot::types::ChatId::new(chat_id),
        &utils::SimpleTemplate::new(msg_tplt)
            .apply("name", waer).apply("count", count).result())).await
        .map_err(|_| error!(RuntimeError, "Failed to send best waer"))?;
    Ok(())
}

pub async fn sendBestWaable(
    api: &bot::Api, chat_id: i64, time_period: chrono::Duration, msg_tplt: &str)
    -> Result<(), Error>
{
    let (waable, count) = chat_db::bestWaable(time_period)?;
    info!("Best waable in the last {}, with {} was.", time_period, count);

    api.send(SendMessage::new(
        bot::types::ChatId::new(chat_id),
        &utils::SimpleTemplate::new(msg_tplt).apply("count", count).result())
             .reply_to(bot::types::MessageId::new(waable))).await
        .map_err(|_| error!(RuntimeError, "Failed to send best waable"))?;
    Ok(())
}
