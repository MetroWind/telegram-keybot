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
