#![allow(non_snake_case)]

use telegram_bot as bot;
use tokio;
use log::info;
use clap;
use chrono;
use chrono::Datelike;

#[macro_use]
mod error;
#[macro_use]
mod utils;
mod reddit;
mod simple_http_server;
mod bot_config;
mod telegram;
mod keybot;
mod chat_db;

use crate::error::Error;

fn readConfig() -> Result<bot_config::ConfigParams, error::Error>
{
    let conf_file = "keybot.toml";
    info!("Reading config from {}...", conf_file);
    bot_config::ConfigParams::fromFile(conf_file)
}

fn lastMonth(from_time: &chrono::DateTime<chrono::Utc>)
             -> Result<chrono::DateTime<chrono::Utc>, error::Error>
{
    let err = error!(RuntimeError, "Failed to get last month");
    let month = from_time.month();
    if month > 1
    {
        from_time.with_month(month - 1).ok_or(err)
    }
    else
    {
        from_time.with_year(from_time.year() - 1).ok_or(err.clone())?
            .with_month(12).ok_or(err)
    }
}

#[tokio::main]
async fn main() -> Result<(), error::Error>
{
    env_logger::init();
    let opts = clap::App::new("Keybot")
        .version("0.1.0")
        .author("@MetroWind")
        .about("A bot for a certain Telegram group")
        .subcommand(clap::App::new("send-reddit-best")
                    .about("Send r/mk's best pic today."))
        .subcommand(clap::App::new("send-weekly-waer")
                    .about("Send weekly waer."))
        .subcommand(clap::App::new("send-weekly-waable")
                    .about("Send weekly waable."))
        .subcommand(clap::App::new("send-monthly-waer")
                    .about("Send monthly waer."))
        .subcommand(clap::App::new("send-monthly-waable")
                    .about("Send monthly waable."))
        .get_matches();

    let config = readConfig()?;
    if !keybot::RuntimeInfo::exist()
    {
        keybot::RuntimeInfo::new().save()?;
    }

    match opts.subcommand_name()
    {
        Some("send-reddit-best") =>
        {
            let api = bot::Api::new(&config.general.token);
            keybot::sendBestRedditToday(&api, &config).await?;
            return Ok(());
        },
        Some("send-weekly-waer") =>
        {
            let api = bot::Api::new(&config.general.token);
            return keybot::sendBestWaer(
                &api, config.general.group_id.unwrap(), chrono::Duration::days(7),
                &config.general.weekly_waer_template).await;
        },
        Some("send-weekly-waable") =>
        {
            let api = bot::Api::new(&config.general.token);
            return keybot::sendBestWaable(
                &api, config.general.group_id.unwrap(), chrono::Duration::days(7),
                &config.general.weekly_waable_template).await;
        },
        Some("send-monthly-waer") =>
        {
            let api = bot::Api::new(&config.general.token);
            let now = chrono::Utc::now();
            return keybot::sendBestWaer(
                &api, config.general.group_id.unwrap(), now - lastMonth(&now)?,
                &config.general.monthly_waer_template).await;
        },
        Some("send-monthly-waable") =>
        {
            let api = bot::Api::new(&config.general.token);
            let now = chrono::Utc::now();
            return keybot::sendBestWaable(
                &api, config.general.group_id.unwrap(), now - lastMonth(&now)?,
                &config.general.monthly_waable_template).await;
        },
        None =>
        {
            if !std::path::Path::new(chat_db::DB_FILENAME).exists()
            {
                chat_db::initialize()?;
            }
            keybot::startBot(&config).await;
        },
        _ =>
        {
            return Err(error!(RuntimeError, "Invalid command"));
        },
    }
    Ok(())
}
