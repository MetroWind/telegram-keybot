use std::collections::HashMap;
use std::time;

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
        let now = time::SystemTime::now().duration_since(time::UNIX_EPOCH)
            .map_err(|_| {error!(RedditError, "failed to get time")})?;
        let state_str: String = uuid::Uuid::new_v1(
            uuid::v1::Timestamp::from_unix(
                uuid::v1::Context::new(42), now.as_secs(), now.subsec_nanos()),
            &[1, 2, 3, 4, 5, 6]).map_err(
            |_| { error!(RedditError, "failed to generate UUID") })?
            .to_string();

        let payload_init = [
            ("client_id", client_id),
            ("response_type", "code"),
            ("state", &state_str),
            ("redirect_uri", "http://localhost:31416/"),
            ("duration", "temporary"),
            ("scope", "identity edit flair history mysubreddits \
privatemessages read report save submit subscribe vote \
wikiedit wikiread")];

        let perm_url = reqwest::Url::parse_with_params(
            "https://www.reddit.com/api/v1/authorize", &payload_init).unwrap();
        println!("Please open the following URI in your browser:\n\n{}",
                 perm_url);

        let server = simple_http_server::SimpleHttpServer::new(31416);
        let params = server.handleOne()?;

        if params.contains_key("error")
        {
            return Err(error!(RedditError, format!(
                "Failed to authenticate: {}", params["error"])));
        }

        if params["state"] != state_str
        {
            return Err(error!(RedditError, "Invalid state string"));
        }

        let code = &params["code"];

        let payload_token = [
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", "http://localhost:31416/")];

        let client = requests::Client::new();
        let res = client.post("https://www.reddit.com/api/v1/access_token")
            .header("User-Agent", USER_AGENT)
            .form(&payload_token).basic_auth(client_id, Some(client_secret))
            .send().map_err(|_| {
                error!(RedditError, "Failed to post token request")})?;

        res.error_for_status_ref().map_err(|e| {
            error!(RedditError, format!("Failed to get token: {}", e))})?;

        let data: HashMap<String, serde_json::Value> = res.json().map_err(|e| {
            error!(RedditError, format!("Failed to parse token response: {}", e))
        })?;

        Ok(Self {
            token: String::from(data["access_token"].as_str().unwrap()),
            refresh_token: Some(String::from(
                data["refresh_token"].as_str().unwrap())),
            client: client,
        })
    }
}
