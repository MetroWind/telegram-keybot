use std::net::{SocketAddr, TcpListener, TcpStream};
use std::io::{Read, Write};
use std::collections::HashMap;
use std::str;

use reqwest::Url;
use regex::bytes::Regex;

use crate::error::Error;

pub struct SimpleHttpServer
{
    port: u16,
}

pub enum HttpMethod
{
    GET,
    POST,
}

pub fn queryFromRequest(req: &[u8]) -> Result<HashMap<String, String>, Error>
{
    let pattern = Regex::new(r"^GET (.*) HTTP/1\.1").unwrap();
    let caps = pattern.captures(req)
        .ok_or(error!(HttpServerError, "invalid request"))?;
    let urlstr: &[u8] = caps.get(1).unwrap().as_bytes();
    let urlstr: String = "http://localhost".to_owned() +
        str::from_utf8(urlstr).unwrap();
    let url = Url::parse(&urlstr).map_err(
        |_| { error!(RuntimeError, format!("invalid URL: {}", urlstr)) })?;
    let mut params: HashMap<String, String> = HashMap::new();
    for pair in url.query_pairs()
    {
        params.insert(pair.0.into_owned(), pair.1.into_owned());
    }
    Ok(params)
}

impl SimpleHttpServer
{
    pub fn new(port: u16) -> Self
    {
        Self { port: port }
    }

    pub fn start(&self) -> Result<(), Error>
    {
        let listener = TcpListener::bind(
            SocketAddr::from(([127,0,0,1], self.port))).unwrap();
        println!("Listening on port {}.", self.port);
        println!("Server starting at http://localhost:{}/ ...", self.port);

        for stream in listener.incoming()
        {
            let stream = stream.unwrap();
            self.handleConnection(stream)?;
        }
        Ok(())
    }

    pub fn handleOne(&self) -> Result<HashMap<String, String>, Error>
    {
        let listener = TcpListener::bind(
            SocketAddr::from(([127,0,0,1], self.port))).unwrap();
        println!("Listening on port {}.", self.port);
        println!("Server starting at http://localhost:{}/ ...", self.port);

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

        let params = queryFromRequest(&buffer)?;

        let contents = "It's a trap!";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            contents.len(), contents);

        stream.write(response.as_bytes()).unwrap();
        stream.flush().unwrap();

        Ok(params)
    }
}
