use std::net::TcpListener;
use std::net::TcpStream;
use std::io::{Read, Write};

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
    pub fn start(&self)
    {
        let listener = TcpListener::bind("127.0.0.1:8000").unwrap();
        println!("Listening on port 8000.");
        println!("Server starting at http://localhost:8000/ ...");

        for stream in listener.incoming()
        {
            let stream = stream.unwrap();
            self.handleConnection(stream);
        }
    }

    fn handleConnection(&self, mut stream: TcpStream) -> Result<(), Error>
    {
        let mut buffer = [0; 1024];
        if let Err(_) = stream.read(&mut buffer)
        {
            return Err(error!(HttpServerError, "Failed to read stream"));
        }

        let get = b"GET / HTTP/1.1\r\n";

        if buffer.starts_with(get)
        {
            let contents = "aaa";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                contents.len(),
                contents
            );

            stream.write(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
        else
        {
            unimplemented!();
        }

        Ok(())
    }
}
