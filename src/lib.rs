use serde_yaml::{Mapping, Value};
use std::borrow::{Borrow, Cow};
use std::collections::VecDeque;
use std::io::Read;

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
    SerDe(serde_yaml::Error),
    UTF8(std::str::Utf8Error),
    ParseInt(std::num::ParseIntError),
    ParseError,
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
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub struct EscapePattern<'a>(pub Cow<'a, [u8]>, pub Cow<'a, [u8]>);
impl Default for EscapePattern<'static> {
    fn default() -> Self {
        EscapePattern(b"{{"[..].into(), b"}}"[..].into())
    }
}
impl<'a> std::str::FromStr for EscapePattern<'a> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split("var");
        let res = EscapePattern(
            split
                .next()
                .ok_or(Error::InvalidEscapeSequence)?
                .as_bytes()
                .to_owned()
                .into(),
            split
                .next()
                .ok_or(Error::InvalidEscapeSequence)?
                .as_bytes()
                .to_owned()
                .into(),
        );
        if res.0 == res.1 {
            Err(Error::InvalidEscapeSequence)
        } else {
            Ok(res)
        }
    }
}
impl<'a> std::fmt::Display for EscapePattern<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}var{}",
            std::str::from_utf8(self.0.borrow()).unwrap(),
            std::str::from_utf8(self.1.borrow()).unwrap(),
        )
    }
}

pub fn get_val_from_config_seq(seq: &[Value], key: &str) -> Option<Value> {
    let mut seg_iter = key.splitn(2, ".");
    let seg = seg_iter.next()?;
    let val = seq.get(seg.parse::<usize>().ok()?)?;
    match (val, seg_iter.next()) {
        (Value::Mapping(m), Some(rest)) => get_val_from_config_map(&m, rest),
        (Value::Sequence(s), Some(rest)) => get_val_from_config_seq(s.as_slice(), rest),
        (Value::Null, _) => None,
        (_, None) => Some(val.clone()),
        _ => None,
    }
}

pub fn get_val_from_config_map(map: &Mapping, key: &str) -> Option<Value> {
    let mut seg_iter = key.splitn(2, ".");
    let seg = seg_iter.next()?;
    let val = map.get(&Value::String(seg.to_owned()))?;
    match (val, seg_iter.next()) {
        (Value::Mapping(m), Some(rest)) => get_val_from_config_map(&m, rest),
        (Value::Sequence(s), Some(rest)) => get_val_from_config_seq(s.as_slice(), rest),
        (Value::Null, _) => None,
        (_, None) => Some(val.clone()),
        _ => None,
    }
}

pub fn set_val_in_config_seq(seq: &mut [Value], key: &str, value: Value) -> Option<()> {
    let mut seg_iter = key.splitn(2, ".");
    let seg = seg_iter.next()?;
    let val = seq.get_mut(seg.parse::<usize>().ok()?)?;
    match (val, seg_iter.next()) {
        (Value::Mapping(m), Some(rest)) => set_val_in_config_map(m, rest, value),
        (Value::Sequence(s), Some(rest)) => set_val_in_config_seq(s.as_mut_slice(), rest, value),
        (val, None) => {
            *val = value;
            Some(())
        }
        _ => None,
    }
}

pub fn set_val_in_config_map(map: &mut Mapping, key: &str, value: Value) -> Option<()> {
    let mut seg_iter = key.splitn(2, ".");
    let seg = seg_iter.next()?;
    let val = map.get_mut(&Value::String(seg.to_owned()))?;
    match (val, seg_iter.next()) {
        (Value::Mapping(m), Some(rest)) => set_val_in_config_map(m, rest, value),
        (Value::Sequence(s), Some(rest)) => set_val_in_config_seq(s.as_mut_slice(), rest, value),
        (val, None) => {
            *val = value;
            Some(())
        }
        _ => None,
    }
}

pub fn val_to_string(val: Value) -> String {
    match val {
        Value::Bool(b) => format!("{}", b),
        Value::Mapping(_) => "{map}".to_owned(),
        Value::Null => "null".to_owned(),
        Value::Number(n) => format!("{}", n),
        Value::Sequence(_) => "[list]".to_owned(),
        Value::String(s) => s,
    }
}

pub fn val_is_truthy(val: &Value) -> bool {
    match val {
        Value::Bool(false) => false,
        Value::Null => false,
        _ => true,
    }
}

/// rpcpassword={{rpcpassword}}
/// {{#IF rpcauth
/// rpcuser={{rpcauth.rpcuser}}
/// rpcpassword={{rpcauth.rpcpassword}}
/// }}
/// {{#IF listen
/// listen=1
/// bind=0.0.0.0:8333
/// }}
/// {{#IFNOT listen
/// listen=0
/// }}
/// {{#FOREACH rpcallowip
/// rpcallowip={{rpcallowip}}
/// }}
pub fn eval(
    map: &Mapping,
    expr: &str,
    escape: &EscapePattern,
    unescape: u8,
) -> Result<String, Error> {
    let trimmed = expr.trim();
    if trimmed.starts_with("#IF ") {
        let mut split = trimmed[4..].splitn(2, "\n");
        let if_var = split.next().ok_or_else(|| Error::ParseError)?.trim();
        let rest = split.next().ok_or_else(|| Error::ParseError)?;
        if get_val_from_config_map(map, if_var)
            .map(|a| val_is_truthy(&a))
            .unwrap_or(false)
        {
            let mut ret = String::new();
            TemplatingReader::new(std::io::Cursor::new(rest.as_bytes()), map, escape, unescape)
                .read_to_string(&mut ret)?;
            Ok(ret)
        } else {
            Ok("".to_owned())
        }
    } else if trimmed.starts_with("#IFNOT ") {
        let mut split = trimmed[4..].splitn(2, "\n");
        let if_var = split.next().ok_or_else(|| Error::ParseError)?.trim();
        let rest = split.next().ok_or_else(|| Error::ParseError)?;
        if !get_val_from_config_map(map, if_var)
            .map(|a| val_is_truthy(&a))
            .unwrap_or(false)
        {
            let mut ret = String::new();
            TemplatingReader::new(std::io::Cursor::new(rest.as_bytes()), map, escape, unescape)
                .read_to_string(&mut ret)?;
            Ok(ret)
        } else {
            Ok("".to_owned())
        }
    } else if trimmed.starts_with("#FOREACH ") {
        let mut split = trimmed[9..].splitn(2, "\n");
        let for_var = split.next().ok_or_else(|| Error::ParseError)?.trim();
        let rest = split.next().ok_or_else(|| Error::ParseError)?;
        match get_val_from_config_map(map, for_var) {
            Some(Value::Sequence(s)) => {
                let mut ret = String::new();
                let mut new_map = map.clone();
                for item in s {
                    set_val_in_config_map(&mut new_map, for_var, item)
                        .ok_or_else(|| Error::MissingConfigValue(for_var.to_owned()))?;
                    TemplatingReader::new(
                        std::io::Cursor::new(rest.as_bytes()),
                        &new_map,
                        escape,
                        unescape,
                    )
                    .read_to_string(&mut ret)?;
                }
                Ok(ret)
            }
            Some(ref a) if val_is_truthy(a) => {
                let mut ret = String::new();
                TemplatingReader::new(std::io::Cursor::new(rest.as_bytes()), map, escape, unescape)
                    .read_to_string(&mut ret)?;
                Ok(ret)
            }
            _ => Ok("".to_owned()),
        }
    } else {
        get_val_from_config_map(map, trimmed)
            .map(val_to_string)
            .ok_or_else(|| Error::MissingConfigValue(trimmed.to_owned()))
    }
}

pub struct TemplatingReader<'a, 'b, 'c, R: Read> {
    inner: R,
    mapping: &'a Mapping,
    escape: &'b EscapePattern<'c>,
    unescape: u8,
    unescapable: bool,
    count_start: usize,
    count_end: usize,
    depth: usize,
    var: Vec<u8>,
    buf: VecDeque<u8>,
}
impl<'a, 'b, 'c, R> TemplatingReader<'a, 'b, 'c, R>
where
    R: Read,
{
    pub fn new(
        reader: R,
        mapping: &'a Mapping,
        escape: &'b EscapePattern<'c>,
        unescape: u8,
    ) -> Self {
        TemplatingReader {
            inner: reader,
            mapping,
            escape,
            unescape,
            unescapable: false,
            count_start: 0,
            count_end: 0,
            depth: 0,
            var: Vec::new(),
            buf: VecDeque::new(),
        }
    }
}

fn to_io_error<E: std::error::Error + Send + Sync + 'static>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e)
}

impl<'a, 'b, 'c, R> Read for TemplatingReader<'a, 'b, 'c, R>
where
    R: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let in_bytes = self.inner.read(buf)?;
        for byte in &buf[..in_bytes] {
            let byte_arr = [*byte];
            let mut to_extend: &[u8] = &byte_arr;
            if self.unescapable && byte == &self.unescape {
                self.depth -= 1;
                self.count_start = 0;
                to_extend = &*self.escape.0;
            }
            self.unescapable = false;
            if byte == &self.escape.0[self.count_start] {
                self.count_start += 1;
                to_extend = &[];
            } else if self.count_start != 0 {
                to_extend = &self.escape.0[..self.count_start];
            }
            if self.depth > 0 && byte == &self.escape.1[self.count_end] {
                self.count_end += 1;
                to_extend = &[];
            } else if self.count_end != 0 {
                to_extend = &self.escape.1[..self.count_end];
            }
            if self.count_start == self.escape.0.len() {
                self.depth += 1;
                self.count_start = 0;
                self.unescapable = true;
            }
            if self.count_end == self.escape.0.len() {
                self.depth -= 1;
                self.count_end = 0;
                if self.depth == 0 {
                    self.buf.extend(
                        eval(
                            self.mapping,
                            std::str::from_utf8(&self.var).map_err(to_io_error)?,
                            self.escape,
                            self.unescape,
                        )
                        .map_err(to_io_error)?
                        .as_bytes(),
                    );
                    self.var.clear();
                }
            }
            if self.depth == 0 {
                self.buf.extend(to_extend);
            } else {
                self.var.extend_from_slice(to_extend);
            }
        }
        let written = std::cmp::min(buf.len(), self.buf.len());
        for (i, elem) in self.buf.drain(0..written).enumerate() {
            buf[i] = elem;
        }
        Ok(written)
    }
}
