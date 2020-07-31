macro_rules! map(
    { $($key:expr => $value:expr),+ } => {
        {
            let mut m = ::std::collections::HashMap::new();
            $(
                m.insert($key, $value);
            )+
            m
        }
     };
);

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
        pub enum $name
        {
            $( $sub_name $( = $x )? ,)+
        }

        impl $name
        {
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
