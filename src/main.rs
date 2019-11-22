use serde_yaml::{Mapping, Value};
use std::borrow::Cow;
use std::io::{Read, Write};

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
    SerDe(serde_yaml::Error),
    UTF8(std::str::Utf8Error),
    ParseInt(std::num::ParseIntError),
    InvalidEscapeSequence,
    MissingArgument,
    MissingConfigValue(String),
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}
impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Self {
        Error::UTF8(e)
    }
}
impl From<std::num::ParseIntError> for Error {
    fn from(e: std::num::ParseIntError) -> Self {
        Error::ParseInt(e)
    }
}
impl From<serde_yaml::Error> for Error {
    fn from(e: serde_yaml::Error) -> Self {
        Error::SerDe(e)
    }
}

#[derive(Debug, Clone)]
pub struct EscapePattern<'a>(pub Cow<'a, str>, pub Cow<'a, str>);
impl Default for EscapePattern<'static> {
    fn default() -> Self {
        EscapePattern("{{".into(), "}}".into())
    }
}
impl<'a> std::str::FromStr for EscapePattern<'a> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split("var");
        Ok(EscapePattern(
            split
                .next()
                .ok_or(Error::InvalidEscapeSequence)?
                .to_owned()
                .into(),
            split
                .next()
                .ok_or(Error::InvalidEscapeSequence)?
                .to_owned()
                .into(),
        ))
    }
}
impl<'a> std::fmt::Display for EscapePattern<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}var{}", self.0, self.1)
    }
}

const BUFFER_SIZE: usize = 1024;

pub fn get_val_from_config_seq(seq: &[Value], key: &str) -> Result<String, Error> {
    let err_closure = || Error::MissingConfigValue(key.to_owned());
    let mut seg_iter = key.splitn(2, ".");
    let seg = seg_iter.next().ok_or_else(err_closure)?;
    let val = seq.get(seg.parse::<usize>()?).ok_or_else(err_closure)?;
    match val {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(format!("{}", n)),
        Value::Bool(b) => Ok(format!("{}", b)),
        Value::Mapping(m) => get_val_from_config_map(&m, &seg_iter.next().ok_or_else(err_closure)?)
            .map_err(|_| err_closure()),
        Value::Sequence(s) => {
            get_val_from_config_seq(s.as_slice(), &seg_iter.next().ok_or_else(err_closure)?)
                .map_err(|_| err_closure())
        }
        Value::Null => Err(err_closure()),
    }
}

pub fn get_val_from_config_map(map: &Mapping, key: &str) -> Result<String, Error> {
    let err_closure = || Error::MissingConfigValue(key.to_owned());
    let mut seg_iter = key.splitn(2, ".");
    let seg = seg_iter.next().ok_or_else(err_closure)?;
    let val = map
        .get(&Value::String(seg.to_owned()))
        .ok_or_else(err_closure)?;
    match val {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(format!("{}", n)),
        Value::Bool(b) => Ok(format!("{}", b)),
        Value::Mapping(m) => get_val_from_config_map(&m, &seg_iter.next().ok_or_else(err_closure)?),
        Value::Sequence(s) => {
            get_val_from_config_seq(s.as_slice(), &seg_iter.next().ok_or_else(err_closure)?)
        }
        Value::Null => Err(err_closure()),
    }
}

pub fn fill_template<R: Read, W: Write>(
    escape: &EscapePattern,
    map: &Mapping,
    src: &mut R,
    dst: &mut W,
) -> Result<(), Error> {
    let mut buf = [0; BUFFER_SIZE];
    let mut last_ends_with_n = 0;
    while {
        let bytes = src.read(&mut buf)?;
        let mut data = &buf[..bytes];
        let mut next_ends_with_n = 0;
        for i in 1..escape.0.as_bytes().len() {
            if data.ends_with(&escape.0.as_bytes()[0..i]) {
                next_ends_with_n = i;
            }
        }
        while {
            let idx: Option<isize> = if data.starts_with(&escape.0.as_bytes()[last_ends_with_n..]) {
                Some(-(last_ends_with_n as isize))
            } else {
                std::str::from_utf8(data)?
                    .find(&escape.0.as_ref())
                    .map(|a| a as isize)
            };
            if let Some(idx) = idx {
                if idx > 0 {
                    dst.write(&data[..(idx as usize)])?;
                }
                let start = idx as usize + escape.0.len();
                let mut var_name = String::new();
                loop {
                    if let Some(end) = std::str::from_utf8(data)?.find(&escape.1.as_ref()) {
                        var_name += std::str::from_utf8(&data[start..end])?;
                        dst.write(get_val_from_config_map(&map, &var_name)?.as_bytes())?;
                        data = &data[(end + escape.1.len())..];
                        break;
                    }
                    var_name += std::str::from_utf8(&data[start..])?;
                    let bytes = src.read(&mut buf)?;
                    data = &buf[..bytes];
                }
                true
            } else {
                dst.write(data)?;
                false
            }
        } {}

        last_ends_with_n = next_ends_with_n;

        bytes == BUFFER_SIZE
    } {}
    Ok(())
}

fn main() -> Result<(), Error> {
    let (escape, map_file) = {
        let mut args = std::env::args();
        let arg = args.next().ok_or(Error::MissingArgument)?;
        if arg == "--template" || arg == "-t" {
            let escape = args.next().ok_or(Error::MissingArgument)?.parse()?;
            let map_file = args.next().ok_or(Error::MissingArgument)?;
            (escape, map_file)
        } else {
            (Default::default(), arg)
        }
    };
    let map: Mapping = serde_yaml::from_reader(std::fs::File::open(map_file)?)?;
    fill_template(&escape, &map, &mut std::io::stdin(), &mut std::io::stdout())?;

    Ok(())
}
