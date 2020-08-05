use std::vec::Vec;
use std::collections::HashMap;
use std::time;
use std::fmt;

// use log::info;
use reqwest;
use reqwest::blocking as requests;
use serde_json;
use uuid;
use chrono::prelude::*;

use crate::error::Error;
use crate::simple_http_server;

#[allow(dead_code)]
pub struct RedditQuerier
{
    token: String,
    refresh_token: Option<String>,
    client_id: String,
    client_secret: String,
}

impl RedditQuerier
{
    const URL_BASE: &'static str = "https://oauth.reddit.com";
    const USER_AGENT: &'static str = "desktop:org.darksair.keybot:0.0.1 (by /u/darksair)";

    pub async fn fromUserlessAuthentication(
        client_id: &str, client_secret: &str) -> Result<Self, Error>
    {
        let url = "https://www.reddit.com/api/v1/access_token";
        let payload = [("grant_type", "client_credentials"),];
        let client = reqwest::Client::new();
        let res = match client.post(url).header("User-Agent", Self::USER_AGENT)
            .form(&payload).basic_auth(client_id, Some(client_secret))
            .send().await
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

        let data: HashMap<String, serde_json::Value> = res.json().await.map_err(|e| {
            error!(RedditError, format!("Failed to authenticate userless: {}", e))
        })?;

        Ok(Self {
            token: String::from(data["access_token"].as_str().unwrap()),
            refresh_token: None,
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
        })
    }

    #[allow(dead_code)]
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
            .header("User-Agent", Self::USER_AGENT)
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
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
        })
    }

    pub fn urlPreprocess(url_raw: &str) -> Result<String, Error>
    {
        let url = reqwest::Url::parse(&("http://localhost".to_owned() + url_raw))
            .map_err(|_| error!(
                RedditError, format!("Failed to parse url {}", url_raw)))?;
        let mut result: String;
        if url.query().is_none()
        {
            result = url_raw.to_owned() + "?raw_json=1";
        }
        else
        {
            result = url_raw.to_owned() + "&raw_json=1";
        }

        if !url_raw.starts_with(Self::URL_BASE)
        {
            result = Self::URL_BASE.to_owned() + &result;
        }
        Ok(result)
    }

    pub async fn query(&self, req_builder: reqwest::RequestBuilder)
                       -> Result<reqwest::Response, Error>
    {
        let mut result = req_builder.header("User-Agent", Self::USER_AGENT);
        result = result.header(
            "Authorization",
            "bearer ".to_owned() + self.token.as_ref());
        let res = result.send().await.map_err(
            |e| {error!(RedditError,
                        format!("Failed to send request: {}", e))})?;
        res.error_for_status().map_err(
            |e| {error!(RedditError, format!("Query failed: {}", e))})
    }

    pub async fn logout(self) -> Result<(), Error>
    {
        let payload = [("token", self.token.as_ref()),
                       ("token_type_hint", "access_token")];
        let client = reqwest::Client::new();
        let res = client.post("https://www.reddit.com/api/v1/revoke_token")
            .header("User-Agent", Self::USER_AGENT)
            .form(&payload).basic_auth(&self.client_id, Some(&self.client_secret))
            .send().await.map_err(|_| {
                error!(RedditError, "Failed to post logout request")})?;

        res.error_for_status().map_err(|e| {
            error!(RedditError, format!("Failed to logout: {}", e))}).map(|_| ())
    }
}

pub struct Post
{
    pub title: String,
    pub text: String,
    pub author: String,
    pub score: i32,
    pub url: String,            // URI to the post itself
    pub link: String,           // Link in the post
    pub hide_score: bool,
    pub id: String,
    pub count_comments: u32,
    pub time_create: DateTime<Utc>,
    pub sub: String,
}

impl Post
{
    #[allow(dead_code)]
    pub fn new() -> Self
    {
        Self {
            title: String::new(),
            text: String::new(),
            author: String::new(),
            score: 0,
            url: String::new(),
            link: String::new(),
            hide_score: false,
            id: String::new(),
            count_comments: 0,
            time_create: Utc.timestamp(0, 0),
            sub: String::new(),
        }
    }

    pub fn fullName(&self) -> &str
    {
        &self.id
    }

    pub fn isLink(&self) -> bool
    {
        if self.link.starts_with(
            &format!("https://www.reddit.com/r/{}/comments", self.sub))
        {
            if self.link["https://www.reddit.com".len()..] == self.url
            {
                return false;
            }
        }
        true
    }

    pub fn shortUrl(&self) -> String
    {
        let uid = self.id.splitn(1, "_").last().unwrap();
        format!("https://reddit.com/r/{}/comments/{}/", self.sub, uid)
    }
}

impl fmt::Debug for Post
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        write!(f, "Post{{{}}}", self.url)
    }
}

makeIntEnum!
{
    PostSorting {hot, new,} with u8,
    derive(Copy, Clone, PartialEq)
}

pub struct Subreddit
{
    name: String
}

impl Subreddit
{
    pub fn new(sub: &str) -> Self
    {
        Self {name: sub.to_owned()}
    }

    fn urlName(&self) -> String
    {
        "/r/".to_owned() + &self.name
    }

    pub async fn list(&self, querier: &RedditQuerier, sorting: PostSorting,
                      before: Option<&str>, after: Option<&str>)
                      -> Result<Vec<Post>, Error>
    {
        let mut query = vec![("g", "GLOBAL"),];
        if let Some(before_id) = before
        {
            query.push(("before", before_id));
        }
        if let Some(after_id) = after
        {
            query.push(("after", after_id));
        }

        let client = reqwest::Client::new();
        let res = querier.query(
            client.get(&RedditQuerier::urlPreprocess(
                &format!("{}/{}.json", self.urlName(), sorting))?)
                .query(&query)).await?;
        let data: serde_json::Value = res.json().await.map_err(
            |_| {error!(RedditError, "Invalid JSON in listing")})?;
        // println!("{}", serde_json::to_string_pretty(&data).unwrap());
        Ok(data["data"]["children"].as_array().unwrap().iter().map(
            |post_data_wrapper|
            {
                let data = &post_data_wrapper["data"];
                Post {
                    title: data["title"].as_str().unwrap().to_owned(),
                    text: data["selftext"].as_str().unwrap().to_owned(),
                    author: data["author"].as_str().unwrap().to_owned(),
                    score: data["score"].as_i64().unwrap() as i32,
                    url: data["permalink"].as_str().unwrap().to_owned(),
                    link: data["url"].as_str().unwrap().to_owned(),
                    hide_score: data["hide_score"].as_bool().unwrap(),
                    id: data["name"].as_str().unwrap().to_owned(),
                    count_comments: data["num_comments"].as_u64().unwrap() as u32,
                    time_create: Utc.timestamp(
                        data["created_utc"].as_f64().unwrap() as i64, 0),
                    sub: data["subreddit"].as_str().unwrap().to_owned(),
                }
            }).collect())
    }
}
