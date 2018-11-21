use bytes::{BufMut, IntoBuf};
use futures_legacy::{Async, Future, Poll};
use http::HeaderMap;
use crate::connect::Connected;
use super::io_err;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Cursor};
use tokio_io::{AsyncRead, AsyncWrite};
use futures_legacy::try_ready;


pub(crate) struct TunnelConnect {
    buf: Vec<u8>,
}

impl TunnelConnect {
    /// Change stream
    pub fn with_stream<S>(self, stream: S, connected: Connected) -> Tunnel<S> {
        Tunnel {
            buf: self.buf.into_buf(),
            stream: Some(stream),
            connected: Some(connected),
            state: TunnelState::Writing,
        }
    }
}

pub(crate) struct Tunnel<S> {
    buf: Cursor<Vec<u8>>,
    stream: Option<S>,
    connected: Option<Connected>,
    state: TunnelState,
}

#[derive(Debug)]
enum TunnelState {
    Writing,
    Reading,
}

struct HeadersDisplay<'a>(&'a HeaderMap);

impl<'a> Display for HeadersDisplay<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        for (key, value) in self.0 {
            let value_str = value.to_str().map_err(|_| fmt::Error)?;
            write!(f, "{}: {}\r\n", key.as_str(), value_str)?;
        }

        Ok(())
    }
}

/// Creates a new tunnel through proxy
pub(crate) fn new(host: &str, port: u16, headers: &HeaderMap) -> TunnelConnect {
    let buf = format!(
        "CONNECT {0}:{1} HTTP/1.1\r\n\
         Host: {0}:{1}\r\n\
         {2}\
         \r\n",
        host,
        port,
        HeadersDisplay(headers)
    ).into_bytes();

    TunnelConnect { buf }
}

impl<S: AsyncRead + AsyncWrite + 'static> Future for Tunnel<S> {
    type Item = (S, Connected);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if self.stream.is_none() || self.connected.is_none() {
            panic!("must not poll after future is complete")
        }

        loop {
            if let TunnelState::Writing = self.state {
                let n = try_ready!(self.stream.as_mut().unwrap().write_buf(&mut self.buf));
                if !self.buf.has_remaining_mut() {
                    self.state = TunnelState::Reading;
                    self.buf.get_mut().truncate(0);
                } else if n == 0 {
                    return Err(io_err("unexpected EOF while tunnel writing"));
                }
            } else {
                let n = try_ready!(
                    self.stream
                        .as_mut()
                        .unwrap()
                        .read_buf(&mut self.buf.get_mut())
                );
                if n == 0 {
                    return Err(io_err("unexpected EOF while tunnel reading"));
                } else {
                    let read = &self.buf.get_ref()[..];
                    if read.len() > 12 {
                        if read.starts_with(b"HTTP/1.1 200") || read.starts_with(b"HTTP/1.0 200") {
                            if read.ends_with(b"\r\n\r\n") {
                                return Ok(Async::Ready((
                                    self.stream.take().unwrap(),
                                    self.connected.take().unwrap().proxy(true),
                                )));
                            }
                            // else read more
                        } else {
                            return Err(io_err("unsuccessful tunnel"));
                        }
                    }
                }
            }
        }
    }
}