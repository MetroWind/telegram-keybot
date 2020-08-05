#![allow(dead_code)]

use std::fmt;
use std::iter::IntoIterator;
use std::convert::AsRef;
use std::ffi::OsStr;
use std::process::Command;

use crate::error::Error;

use regex;

macro_rules! makeIntEnum
{
    {
        $name:ident
        {
            $( $sub_name:ident $( = $x:literal )? ,)+
        } with $underlying_type:ty,
        $( derive( $($traits:ident),+ ) )?
    } => {
        $( #[derive(fmt::Debug, $($traits),+) ] )?
        #[allow(non_camel_case_types)]
        pub enum $name
        {
            $( $sub_name $( = $x )? ,)+
        }

        impl $name
        {
            #[allow(dead_code)]
            pub fn from(x: $underlying_type) -> Result<Self, String>
            {
                match x
                {
                    $( x if x == (Self::$sub_name as $underlying_type) => Ok(Self::$sub_name), )+
                        _ => Err(format!("Unknown convertion from {} to {}", x, stringify!($name))),
                }
            }
        }

        impl fmt::Display for $name
        {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
            {
                let represent = match self
                {
                    $( Self::$sub_name => stringify!($sub_name), )+
                };
                write!(f, "{}", represent)
            }
        }
    }
}

pub fn strip(s: String) -> String
{
    let len = s.len();

    let left_idx = if let Some((l, _)) = &s.char_indices().find(|(_, c)| { !c.is_whitespace() })
    {
        l.clone()
    }
    else
    {
        0
    };

    let right_idx = if let Some((r, _)) = &s.char_indices().rfind(|(_, c)| { !c.is_whitespace() })
    {
        r + 1
    }
    else
    {
        0
    };

    if right_idx - left_idx <= 1
    {
        String::new()
    }
    else if left_idx == 0 && right_idx == len - 1
    {
        s
    }
    else
    {
        String::from(&s[left_idx..right_idx])
    }
}

pub struct SimpleTemplate
{
    tplt: String,
}

impl SimpleTemplate
{
    pub fn new(s: &str) -> Self
    {
        Self {tplt: s.to_string()}
    }

    pub fn apply<ValueType: fmt::Display>(self, key: &str, value: ValueType)
                                          -> Self
    {
        let pattern = regex::Regex::new(&format!(r"\$\{{{}\}}", key)).unwrap();
        Self::new(&pattern.replace_all(&self.tplt, &format!("{}", value)[..])
                  .into_owned())
    }

    pub fn result(self) -> String
    {
        self.tplt
    }
}

#[test]
fn testTemplate()
{
    let t = SimpleTemplate::new("${user}，你已经是一个键盘侠啦！快来和大家打个招呼吧~");
    assert_eq!(&t.apply("user", "abc").result(),
               "abc，你已经是一个键盘侠啦！快来和大家打个招呼吧~");
}

pub fn run<I, S>(command: I) -> Result<(), Error>
    where I: IntoIterator<Item = S>,
          S: AsRef<OsStr> + fmt::Display + Clone
{
    let mut iter = command.into_iter();
    let prog = iter.next().unwrap();
    let status = Command::new(prog.clone()).args(iter).status().map_err(
        |_| error!(RuntimeError, format!("Failed to run {}", prog)))?;

    if status.success()
    {
        Ok(())
    }
    else if let Some(code) = status.code()
    {
        Err(error!(RuntimeError, format!("{} failed with {}", prog, code)))
    }
    else
    {
        Err(error!(RuntimeError, format!("{} was terminated", prog)))
    }
}

pub fn runWithOutput<I, S>(command: I) -> Result<(Vec<u8>, Vec<u8>), Error>
    where I: IntoIterator<Item = S>,
          S: AsRef<OsStr> + fmt::Display + Clone
{
    let mut iter = command.into_iter();
    let prog = iter.next().unwrap();
    let output = Command::new(prog.clone()).args(iter).output().map_err(
        |_| error!(RuntimeError, format!("Failed to run {}", prog)))?;

    if output.status.success()
    {
        Ok((output.stdout, output.stderr))
    }
    else if let Some(code) = output.status.code()
    {
        Err(error!(RuntimeError, format!("{} failed with {}", prog, code)))
    }
    else
    {
        Err(error!(RuntimeError, format!("{} was terminated", prog)))
    }
}
