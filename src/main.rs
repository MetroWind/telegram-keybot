#![allow(non_snake_case)]

use std::io;
use std::io::prelude::*;

#[macro_use]
mod error;
#[macro_use]
mod utils;
mod reddit;
mod simple_http_server;

fn main() -> Result<(), error::Error>
{
    let stdin = io::stdin();
    let mut app_id = String::new();
    let mut app_secret = String::new();

    print!("App ID: ");
    io::stdout().flush();
    stdin.read_line(&mut app_id);
    print!("App secret: ");
    io::stdout().flush();
    stdin.read_line(&mut app_secret);

    let redditor = reddit::RedditQuerier::fromAuthentication(
        &app_id, &app_secret)?;
    Ok(())
}
