use std::error::Error as StdError;
use std::fmt;

#[macro_export]
macro_rules! error
{
    ( $err_type:ident, $msg:expr ) =>
    {
        {
            Error::$err_type(String::from($msg))
        }
    };
}

#[derive(Debug)]
pub enum Error
{
    RuntimeError(String),
    HttpServerError(String),
}

impl fmt::Display for Error
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        match self
        {
            Error::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
            Error::HttpServerError(msg) => write!(f, "HTTP server error: {}", msg),
        }
    }
}

impl StdError for Error
{
    fn source(&self) -> Option<&(dyn StdError + 'static)> {None}
}
