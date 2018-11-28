use std::fmt;
use crate::utils::iter_fn;

fn ltrim(mut val: &[u8]) -> &[u8] {
    while let Some(&b' ') = val.get(0) {
        val = &val[1..]
    }

    val
}

fn rtrim(mut val: &[u8]) -> &[u8] {
    while let Some(&b' ') = val.get(val.len() - 1) {
        val = &val[..val.len() - 2]
    }

    val
}


#[derive(Debug)]
pub enum Header<'a> {
    Status(u16, &'a [u8], &'a [u8]),
    Header(&'a [u8], &'a [u8]),
}

#[derive(Debug)]
pub enum ParseError<'a> {
    WrongStatusReason,
    WrongStatusHeader(&'a [u8]),
    WrongStatusCode(&'a [u8]),
    WrongHeader(&'a [u8]),
}

impl fmt::Display for ParseError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:?}", self)
    }
}

impl std::error::Error for ParseError<'_> {}

fn parse_header(input: &[u8]) -> Result<(&[u8], &[u8]), ParseError> {
    let mut iter = input.split(|x| *x == b':');
    let name = ltrim(rtrim(iter.next().unwrap()));
    let value = iter.next().ok_or_else(|| ParseError::WrongHeader(name))?;

    Ok((name, ltrim(rtrim(value))))
}

fn parse_status(input: &[u8]) -> Result<(u16, &[u8], &[u8]), ParseError> {
    let mut iter = input.split(|i| *i == b' ');

    let x = iter.next()
        .ok_or_else(|| ParseError::WrongStatusHeader(&input[0..0]))?;

    if &x[0..4] != b"HTTP" {
        return Err(ParseError::WrongStatusHeader(&input[0..4]))
    }

    let http_version = &x[5..];

    let code = iter.next()
        .ok_or_else(|| ParseError::WrongStatusCode(&input[0..0]))?;

    if code.len() < 3 {
        return Err(ParseError::WrongStatusCode(code))
    }

    let (a, b, c) = unsafe {
        ((*code.get_unchecked(0) as i16 - b'0' as i16),
         (*code.get_unchecked(1) as i16 - b'0' as i16),
         (*code.get_unchecked(2) as i16 - b'0' as i16))
    };

    if  a < 1 || a > 5 ||
        b < 0 || b > 9 ||
        c < 0 || c > 9 {

        return Err(ParseError::WrongStatusCode(code));
    }

    let reason = iter.next()
        .ok_or_else(|| ParseError::WrongStatusReason)?;

    Ok(((a * 100 + b * 10 + c) as u16, http_version, reason))
}

pub fn parse_headers(mut input: &[u8]) -> impl Iterator<Item = Result<Header<'_>, ParseError>> {
    let mut status_parsed = false;

    iter_fn(move || {
        let idx = input.windows(2).position(|s| &s[0..2] == b"\r\n")?;
        let line = &input[..idx];
        input = &input[idx + 2..];

        if !status_parsed {
            status_parsed = true;
            match parse_status(line) {
                Ok((status, version, reason)) => {
                    Some(Ok(Header::Status(status, version, reason)))
                },
                Err(err) => Some(Err(err)),
            }
        } else if line.len() > 0 {
            match parse_header(line) {
                Ok((name, value)) => {
                    Some(Ok(Header::Header(name, value)))
                },
                Err(err) => Some(Err(err)),
            }
        } else {
            None
        }
    })
}