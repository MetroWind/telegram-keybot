use std::str;
use std::fs;
use std::io::prelude::*;

use base64::prelude::*;
use rand::prelude::*;
use rand::thread_rng;
use log::{info,debug};
use log::error as log_error;
use tokio;
use serde_json;
use serde::{Serialize, Deserialize};
use teloxide::prelude::*;
use teloxide::types::{Message, MessageId, ChatId};
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

async fn welcome(bot: &Bot, config: &bot_config::ConfigParams,
                 new_users: &[teloxide::types::User], chat: &teloxide::types::Chat)
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
        info!("{} joined chat {} ({}).", name, chat.id, chat.title().unwrap_or(""));
        let msg_tplt = utils::SimpleTemplate::new(&config.general.welcome);
        let msg = msg_tplt
            .apply("user", format!("tg://user?id={}",
                                   telegram::getUserFullname(user)))
            .result();
        
        if bot.send_message(chat.id, msg).send().await.is_err()
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

async fn onNewChatMembers(bot: &Bot, config: &bot_config::ConfigParams,
                          new_users: &[teloxide::types::User], chat: &teloxide::types::Chat)
                          -> Result<(), Error>
{
    debug!("New members in chat {} ({}).", chat.id, chat.title().unwrap_or(""));
    // Is the bot one of the new members?
    if new_users.iter().any(|u|
        match &u.username
        {
            Some(nick) => nick == &config.general.username,
            None => false,
        })
    {
        let mut info = RuntimeInfo::load()?;
        if !info.chat_id.contains(&(chat.id.0)) // ChatId(i64)
        {
            info.chat_id.push(chat.id.0);
            if let Err(e) = info.save()
            {
                log_error!("{}", e);
            }
        }
    }
    else if config.general.do_welcome
    {
        welcome(bot, config, new_users, chat).await?;
    }
    Ok(())
}

async fn onWaReply(bot: &Bot, config: &bot_config::ConfigParams, msg: &Message)
                   -> Result<(), Error>
{
    let chat_id = msg.chat.id; // ChatId
    // Sliently ignore if the reply is not sent in the correct chat.
    if let Some(correct_chat) = config.general.group_id
    {
        if chat_id.0 != correct_chat
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

    let waer_id = if let Some(u) = msg.from() {
        u.id.0 as i64 // UserId(u64) -> i64
    } else {
        return Ok(());
    };

    let wa_count = chat_db::addWa(chat_db::WaEntry {
        wa_to: waable_id.0 as i64, // MessageId(i32) -> i64
        id: msg.id.0 as i64,
        waer: waer_id,
        time: chrono::Utc.timestamp_opt(msg.date.timestamp(), 0).single().unwrap(),
    })?;

    debug!("It's a wa. Wa count is {}", wa_count);
    if wa_count == 3
    {
        let delay: f64 = thread_rng().gen_range(10.0..600.0);
        telegram::replyWithDelay(
            bot, "哇！".to_string(), waable_id, chat_id, delay).await?;
    }
    Ok(())
}

async fn onTextReplyToMsg(bot: &Bot, config: &bot_config::ConfigParams,
                          msg: &Message, reply_to: &Message) -> Result<(), Error>
{
    debug!("Reply to {} receivd.", reply_to.id);
    if let Some(text) = msg.text()
    {
        if text.starts_with("哇")
        {
            onWaReply(bot, config, msg).await?;
        }
    }
    Ok(())
}


async fn onTextReply(bot: &Bot, config: &bot_config::ConfigParams,
                     msg: &Message, reply_to: &Message)
                     -> Result<(), Error>
{
    // In teloxide reply_to is always a Message (if present)
    onTextReplyToMsg(bot, config, msg, reply_to).await
}

async fn trySendFirstPhotoFromPosts(bot: &Bot, chat_id: i64,
                              reddit_posts: &Vec<&reddit::Post>,
                              config: &bot_config::ConfigParams) -> Result<Message, Error>
{
    for best_post in reddit_posts
    {
        info!("Best reddit post today is {}, with image at {}.",
              best_post.shortUrl(), &best_post.link);

        if let Ok((msg, local_path)) =
            telegram::sendPhoto(
                bot, &best_post.link, &utils::SimpleTemplate::new(&config.reddit.daily_pic_caption)
                    .apply("url", best_post.shortUrl()).result(),
                chat_id).await
        {
            if let Some(llm_config) = &config.llm {
                // Use the local resized image if available, otherwise use the URL.
                let image_input = if let Some(path) = &local_path {
                    match tokio::fs::read(path).await {
                        Ok(bytes) => {
                            let b64 = BASE64_STANDARD.encode(&bytes);
                            format!("data:image/jpeg;base64,{}", b64)
                        },
                        Err(e) => {
                            log_error!("Failed to read local image for LLM: {}", e);
                            best_post.link.clone()
                        }
                    }
                } else {
                    best_post.link.clone()
                };

                // Generate and send the LLM comment as a reply to the photo.
                match crate::llm::generate_comment(llm_config, &image_input).await {
                    Ok(comment) => {
                         info!("LLM comment generated: {}", comment);
                         if let Err(e) = bot.send_message(ChatId(chat_id), comment)
                                .reply_to_message_id(msg.id).send().await {
                             log_error!("Failed to send LLM comment: {}", e);
                         }
                    },
                    Err(e) => log_error!("Failed to generate LLM comment: {}", e),
                }
            }

            // Cleanup local file if it exists
            if let Some(path) = local_path {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    log_error!("Failed to remove temp file {:?}: {}", path, e);
                }
            }

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

pub async fn sendBestRedditToday(bot: &Bot, config: &bot_config::ConfigParams)
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
        bot, chat_id, &best_posts, config).await?;

    let mut info = RuntimeInfo::load()?;
    info.last_msg_id = Some(msg.id.0 as i64);
    info.wa_count = 0;
    info.save()?;
    Ok(msg)
}

async fn onMessage(bot: &Bot, config: &bot_config::ConfigParams, msg: Message)
                   -> Result<(), Error>
{
    if let Some(text) = msg.text() {
        if let Some(reply_to) = msg.reply_to_message()
        {
            onTextReply(bot, config, &msg, reply_to).await?;
        }
    }
    
    // Check new chat members
    if let Some(members) = msg.new_chat_members() {
         onNewChatMembers(bot, config, members, &msg.chat).await?;
    }
    
    Ok(())
}

async fn onChannelPost(bot: &Bot, config: &bot_config::ConfigParams,
                       post: Message) -> Result<(), Error>
{
    // Channel posts are Messages in teloxide
    if let Some(members) = post.new_chat_members() {
        onNewChatMembers(bot, config, members, &post.chat).await?;
    }
    Ok(())
}

pub async fn startBot(config: &bot_config::ConfigParams)
{
    let bot = Bot::new(&config.general.token);
    let mut offset: Option<i32> = None;
    
    info!("Entering update loop...");
    loop {
        // Simple long polling loop
        // We use reqwest directly implicitly via teloxide
        
        // We need to construct a params object for get_updates, or just use method builder
        let mut req = bot.get_updates();
        if let Some(o) = offset {
            req = req.offset(o);
        }
        
        let updates = match req.send().await {
            Ok(u) => u,
            Err(e) => {
                log_error!("Failed to fetch updates: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        for update in updates {
            offset = Some(update.id + 1);
            let bot = bot.clone();
            let config = config.clone();

            tokio::spawn(async move {
                match update.kind {
                    teloxide::types::UpdateKind::Message(msg) => {
                         if let Err(e) = onMessage(&bot, &config, msg).await {
                             log_error!("{}", e);
                         }
                    },
                    teloxide::types::UpdateKind::ChannelPost(post) => {
                        if let Err(e) = onChannelPost(&bot, &config, post).await {
                            log_error!("{}", e);
                        }
                    },
                    _ => {},
                }
            });
        }
    }
}

pub async fn sendBestWaer(
    bot: &Bot, chat_id: i64, time_period: chrono::Duration, msg_tplt: &str)
    -> Result<(), Error>
{
    let (waer, count) = chat_db::bestWaer(time_period)?;
    info!("Best waer the last {}, with {} was.", time_period, count);

    // waer is i64, need to cast to u64 for UserId
    let username = telegram::getUserFullname(
        &telegram::getChatMember(bot, chat_id, waer).await?);

    bot.send_message(
        ChatId(chat_id),
        &utils::SimpleTemplate::new(msg_tplt)
            .apply("name", username).apply("count", count).result()).send().await
        .map_err(|_| error!(RuntimeError, "Failed to send best waer"))?;
    Ok(())
}

pub async fn sendBestWaable(
    bot: &Bot, chat_id: i64, time_period: chrono::Duration, msg_tplt: &str)
    -> Result<(), Error>
{
    let (waable, count) = chat_db::bestWaable(time_period)?;
    info!("Best waable in the last {}, with {} was.", time_period, count);

    // waable is i64, cast to i32 for MessageId
    bot.send_message(
        ChatId(chat_id),
        &utils::SimpleTemplate::new(msg_tplt).apply("count", count).result())
             .reply_to_message_id(MessageId(waable as i32)).send().await
        .map_err(|_| error!(RuntimeError, "Failed to send best waable"))?;
    Ok(())
}
