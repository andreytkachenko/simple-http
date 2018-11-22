use futures::stream::Stream;
use std::io;
use std::pin::Pin;
use std::task::{Poll, LocalWaker};
use tokio::prelude::Async;


pub struct Response<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub(crate) status_code: u16,
    pub(crate) status_reason: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: B,
}

impl<B> Response<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub fn new(status_code: u16, status_reason: String, headers: Vec<(String, String)>, body: B) -> Self {
        Self {
            status_code,
            status_reason,
            headers,
            body,
        }
    }

    pub fn get_header<'a>(&'a self, name: & str) -> Option<&'a str> {
        self.headers.iter().find_map(|(n, v)| {
            if n == name { Some(v.as_str()) } else { None }
        })
    }

    pub fn code(&self) -> u16 {
        self.status_code
    }

    pub fn into_body(self) -> B {
        self.body
    }
}

pub struct Body {
    failed: bool,
    rest: Option<Vec<u8>>,
    buf: Vec<u8>,
    counter: usize,
    content_length: Option<usize>,
    reader: Box<dyn tokio_io::AsyncRead + Send + 'static>,
}

impl Body {
    pub fn new(reader: impl tokio_io::AsyncRead + Send + 'static, rest: Option<Vec<u8>>, content_length: Option<usize>) -> Self {
        let mut buf = Vec::with_capacity(4096);
        unsafe {buf.set_len(4096)};

        Body {
            failed: false,
            rest,
            buf,
            counter: 0,
            content_length,
            reader: Box::new(reader)
        }
    }
}

impl Stream for Body {
    type Item = io::Result<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, _lw: &LocalWaker) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        if this.failed {
            return Poll::Ready(None);
        }

        if let Some(vec) = this.rest.take() {
            Poll::Ready(Some(Ok(vec)))
        } else {
            match this.reader.poll_read(&mut this.buf[0 ..]) {
                Ok(Async::NotReady) => Poll::Pending,
                Ok(Async::Ready(mut n)) => {
                    this.counter += n;

                    if let Some(max) = this.content_length {
                        if this.counter > max {
                            n -= this.counter - max;
                        }
                    }

                    if n > 0 {
                        Poll::Ready(Some(Ok(this.buf[0 .. n].to_vec())))
                    } else {
                        Poll::Ready(None)
                    }
                },
                Err(err) => {
                    this.failed = true;
                    Poll::Ready(Some(Err(err)))
                }
            }
        }
    }
}