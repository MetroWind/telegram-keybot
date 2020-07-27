#![allow(non_snake_case)]

#[macro_use]
mod error;

mod simple_http_server;

fn main()
{
    let server = simple_http_server::SimpleHttpServer{};
    server.start();
}
