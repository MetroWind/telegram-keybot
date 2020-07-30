use std::net::TcpListener;
use std::net::TcpStream;
use std::io::{Read, Write};
use std::collections::HashMap;
use std::str;

use reqwest::Url;
use regex::bytes::Regex;

use crate::error::Error;

pub struct SimpleHttpServer
{
}

pub enum HttpMethod
{
    GET,
    POST,
}

impl SimpleHttpServer
{
    pub fn start(&self) -> Result<(), Error>
    {
        let listener = TcpListener::bind("127.0.0.1:8000").unwrap();
        println!("Listening on port 8000.");
        println!("Server starting at http://localhost:8000/ ...");

        for stream in listener.incoming()
        {
            let stream = stream.unwrap();
            self.handleConnection(stream)?;
        }
        Ok(())
    }

    pub fn handleOne(&self) -> Result<HashMap<String, String>, Error>
    {
        let listener = TcpListener::bind("127.0.0.1:8000").unwrap();
        println!("Listening on port 8000.");
        println!("Server starting at http://localhost:8000/ ...");

        for stream in listener.incoming()
        {
            let stream = stream.unwrap();
            return self.handleConnection(stream);
        }
        Err(error!(HttpServerError, "Failed to get params"))
    }

    fn handleConnection(&self, mut stream: TcpStream) -> Result<HashMap<String, String>, Error>
    {
        let mut buffer = [0; 65536];
        stream.read(&mut buffer).map_err(|_| {
            error!(HttpServerError, "Failed to read stream") })?;

        // let request = str::from_utf8(&buffer);
        println!("Request:\n{}", str::from_utf8(&buffer).unwrap());
        let get = b"GET / HTTP/1.1\r\n";
        let pattern = Regex::new(r"(?-u)^GET (.*) HTTP/1\.1.*$").unwrap();
        let caps = pattern.captures(&buffer)
            .ok_or(error!(HttpServerError, "invalid request"))?;
        let urlstr: &[u8] = caps.get(1).unwrap().as_bytes();
        let urlstr: String = "http://localhost:8000".to_owned() +
            str::from_utf8(urlstr).unwrap();
        let url = Url::parse(&urlstr).map_err(
            |_| { error!(RuntimeError, format!("invalid URL: {}", urlstr)) })?;
        let mut params: HashMap<String, String> = HashMap::new();
        for pair in url.query_pairs()
        {
            params.insert(pair.0.into_owned(), pair.1.into_owned());
        }

        let contents = "It's a trap!";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            contents.len(), contents);

        stream.write(response.as_bytes()).unwrap();
        stream.flush().unwrap();

        Ok(params)
    }
}
