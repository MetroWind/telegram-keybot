use std::collections::HashMap;

use log::info;
use reqwest;
use reqwest::blocking as requests;
use serde_json;
use uuid;

use crate::error::Error;
use crate::simple_http_server;

static URL_BASE_AUTH: &str = "https://oauth.reddit.com";
static USER_AGENT: &str = "desktop:org.darksair.keybot:0.0.1 (by /u/darksair)";

pub struct RedditQuerier
{
    token: String,
    refresh_token: Option<String>,
    client: requests::Client,
}

impl RedditQuerier
{
    pub fn fromUserlessAuthentication(
        client_id: &str, client_secret: &str) -> Result<Self, Error>
    {
        let url = "https://www.reddit.com/api/v1/access_token";
        let payload = [("grant_type", "client_credentials"),];
        let client = requests::Client::new();
        let res = match client.post(url).header("User-Agent", USER_AGENT)
            .form(&payload).basic_auth(client_id, Some(client_secret))
            .send()
        {
            Err(_) => { return Err(error!(
                RedditError, "Failed to authenticate userless (post)")); },
            Ok(res) => res,
        };

        if let Err(e) = res.error_for_status_ref()
        {
            return Err(error!(
                RedditError, format!("Failed to authenticate userless ({})",
                                     e.status().unwrap().as_u16())));
        }

        let data: HashMap<String, serde_json::Value> = res.json().map_err(|e| {
            error!(RedditError, format!("Failed to authenticate userless: {}", e))
        })?;

        Ok(Self {
            token: String::from(data["access_token"].as_str().unwrap()),
            refresh_token: None,
            client: client,
        })
    }

    pub fn fromAuthentication(client_id: &str, client_secret: &str)
                              -> Result<Self, Error>
    {
        let state_str: String = uuid::Uuid::new_v1(
            uuid::v1::Timestamp::from_unix(uuid::v1::Context::new(42),
                                           1497624119, 1234),
            &[1, 2, 3, 4, 5, 6]).map_err(
            |_| { error!(RuntimeError, "failed to generate UUID") })?
            .to_string();

        let init_payload = [
            ("client_id", client_id),
            ("response_type", "code"),
            ("state", &state_str),
            ("redirect_uri", "http://localhost:8000/"),
            ("duration", "temporary"),
            ("scope", "identity edit flair history mysubreddits \
privatemessages read report save submit subscribe vote \
wikiedit wikiread")];

        let perm_url = reqwest::Url::parse_with_params(
            "https://www.reddit.com/api/v1/authorize", &init_payload).unwrap();
        println!("Please open the following URI in your browser:\n\n{}",
                 perm_url);

        let server = simple_http_server::SimpleHttpServer{};
        let params = server.handleOne();

        println!("{:?}", params);

        Ok(Self {
            token: String::new(),
            refresh_token: None,
            client: requests::Client::new(),
        })

    }
}
