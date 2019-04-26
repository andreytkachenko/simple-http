use futures::stream::Stream;
use std::task::{Poll, Context};
use std::pin::Pin;
use std::io;
use futures::io::AsyncRead;
use futures::compat::*;


static EMPTY: &[u8] = &[];

pub struct Body {
    drained: bool,
    rest: Option<Vec<u8>>,
    buf: Vec<u8>,
    counter: usize,
    content_length: Option<usize>,
    reader: Box<dyn AsyncRead + Send + 'static>,
}

impl Body {
    pub fn empty() -> Self {
        Body {
            drained: true,
            rest: None,
            buf: Vec::new(),
            counter: 0,
            content_length: None,
            reader: Box::new(EMPTY)
        }
    }
    
    pub fn new(reader: impl tokio_io::AsyncRead + Send + 'static, rest: Option<Vec<u8>>, content_length: Option<usize>) -> Self {
        let mut buf = Vec::with_capacity(4096);
        unsafe {buf.set_len(4096)};

        Body {
            drained: false,
            rest,
            buf,
            counter: 0,
            content_length,
            reader: Box::new(reader.compat())
        }
    }
}

impl Stream for Body {
    type Item = io::Result<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        if this.drained {
            return Poll::Ready(None);
        }

        if let Some(vec) = this.rest.take() {
            Poll::Ready(Some(Ok(vec)))
        } else {
            match unsafe {Pin::new_unchecked(&mut *this.reader)}.poll_read(ctx, &mut this.buf[0 ..]) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(mut n)) => {
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
                Poll::Ready(Err(err)) => {
                    this.drained = true;
                    Poll::Ready(Some(Err(err)))
                }
            }
        }
    }
}
