#![allow(non_snake_case)]

use telegram_bot as bot;
use tokio;
use log::info;
use clap;

#[macro_use]
mod error;
#[macro_use]
mod utils;
mod reddit;
mod simple_http_server;
mod keybot;

fn readConfig() -> Result<keybot::ConfigParams, error::Error>
{
    let conf_file = "keybot.toml";
    info!("Reading config from {}...", conf_file);
    keybot::ConfigParams::fromFile(conf_file)
}

#[tokio::main]
async fn main() -> Result<(), error::Error>
{
    env_logger::init();
    let opts = clap::App::new("Keybot")
        .version("0.1.0")
        .author("@MetroWind")
        .about("A bot for a certain Telegram group")
        .arg(clap::Arg::with_name("send-reddit-best")
             .long("--send-reddit-best")
             .help("Send r/mk's best pic today.")
             .takes_value(false))
        .get_matches();

    let config = readConfig()?;
    if !keybot::RuntimeInfo::exist()
    {
        keybot::RuntimeInfo::new().save()?;
    }

    if opts.is_present("send-reddit-best")
    {
        let api = bot::Api::new(&config.general.token);
        keybot::sendBestRedditToday(&api, &config).await?;
        return Ok(());
    }

    keybot::startBot(&config).await;
    Ok(())
}
