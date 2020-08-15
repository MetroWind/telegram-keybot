use std::fs;
use std::io::prelude::*;

use serde::{Serialize, Deserialize};

use crate::error::Error;

pub static TG_IMG_SIZE_LIMIT: u32 = 4096;
pub static TG_IMG_FILE_SIZE_LIMIT: u64 = 5 * 1024 * 1024;
pub static IMG_RESIZE_TARGET: u32 = 1024;
pub static IMG_RESIZE_QUALITY: u32 = 92;

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
