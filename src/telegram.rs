use std::str;
use std::fs;
use std::time;
use std::path::PathBuf;

use log::{info,debug};
use tokio;
use telegram_bot as bot;
use telegram_bot::types::Message;
use telegram_bot::types::requests::{SendMessage, SendPhoto};
use reqwest;
use reqwest::header::CONTENT_LENGTH;
use tempfile;

use crate::error::Error;
use crate::utils;
use crate::bot_config;

pub async fn replyWithDelay(api: &bot::Api, msg: String,
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
}

pub fn getUserFullname(u: &bot::User) -> String
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

pub fn getUsername(u: &bot::User) -> String
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

/// If `msg` is a reply to a message that is not a channel post,
/// return the message id to which `msg` replies.
pub fn getParentMsgId(msg: &Message) -> Option<bot::MessageId>
{
    if let Some(reply_to) = &msg.reply_to_message
    {
        if let bot::types::MessageOrChannelPost::Message(m) = reply_to.as_ref()
        {
            return Some(m.id);
        }
    }
    None
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

pub async fn sendPhoto(api: &bot::Api, uri: &str, caption: &str, chat_id: i64)
                       -> Result<Message, Error>
{
    debug!("Sending photo at {}...", uri);
    let size = getImageSize(uri)?;
    debug!("Image size is {}x{}.", size.0, size.1);
    let file_info = getUriFileSize(uri).await?;
    // debug!("File size for {} is {}.", file_info.filename, file_info.size);
    if size.0 < bot_config::TG_IMG_SIZE_LIMIT &&
        size.1 < bot_config::TG_IMG_SIZE_LIMIT &&
        file_info.size < bot_config::TG_IMG_FILE_SIZE_LIMIT
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
        info!("Resizing image to {}...", bot_config::IMG_RESIZE_TARGET);
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
              "-resize", &format!("{s}x{s}", s=bot_config::IMG_RESIZE_TARGET),
              "-quality", &bot_config::IMG_RESIZE_QUALITY.to_string(),
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
