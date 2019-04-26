#![feature(await_macro, async_await, futures_api)]

pub mod http;
pub mod https;
pub mod proxy;
mod request;
mod response;
mod httparse;
mod connect;
mod body;

use std::{io, mem};
use std::pin::Pin;
use futures::stream::{Stream, StreamExt};
pub use self::response::Response;
pub use self::body::Body;
pub use self::request::Request;
use self::httparse::{parse_headers, Header};

pub use hyper::Uri;
use self::connect::{Connect, Destination};
pub use self::https::HttpsConnector;
pub use self::connect::HttpConnector;
pub use ::http::Method;
use std::marker::PhantomData;
use futures::compat::*;

pub struct Client<C>
    where C: Connect<Error=io::Error>,
{
    inner: C
}

impl<C> Client<C>
    where C: Connect<Error=io::Error>,
{
    pub fn new(inner: C) -> Self {
        Self { inner }
    }

    pub fn builder() -> ClientBuilder<C> {
        ClientBuilder {
            m: Default::default()
        }
    }

    pub async fn request<'a, B>(&'a self, mut req: Request<B>) -> io::Result<Response<Body>>
        where B: Stream<Item = io::Result<Vec<u8>>> + Send + 'a
    {
        let (conn, _)= await!(self.inner.connect(Destination {
            uri: req.uri.clone()
        }).compat())?;

        // sending headers
        let header = self.build_req(req.method, req.uri, req.headers);

        let (mut conn, _) = await!(tokio_io::io::write_all(conn, header.as_bytes()).compat())?;

        // sending body
        if let Some(mut body) = req.body.take() {
            let mut x = unsafe {Pin::new_unchecked(&mut body)};

            while let Some(res) = await!(x.next()) {
                let (c, _) = await!(tokio_io::io::write_all(conn, res?).compat())?;
                conn = c
            }
        }

        // receiving headers
        let mut buf: [u8; 4096] = unsafe { mem::uninitialized() };
        let mut left = 0usize;

        let (header, rest) = loop {
            let (tconn, _, len) = await!(tokio_io::io::read(conn, &mut buf[left ..]).compat())?;
            conn = tconn;

            if len == 0 {
                return Err(io::Error::new(io::ErrorKind::Other, "Broken headers".to_string()));
            }

            left += len;

            if let Some(mut idx) = buf[..left].windows(4).position(|s| s == b"\r\n\r\n") {
                idx += 4;
                let mut status = 0u16;
                let mut headers = Vec::new();
                let mut reason = String::new();

                for res in parse_headers(&buf[0 .. idx]) {
                    let item = res.map_err(|_err| io::Error::new(io::ErrorKind::Other, "Parse header error"))?;

                    match item {
                        Header::Status(code, r, _) => {
                            status = code;
                            reason = String::from_utf8_lossy(r).to_string();
                        },
                        Header::Header(name, value) => headers.push((
                            String::from_utf8_lossy(name).to_lowercase(),
                            String::from_utf8_lossy(value).to_string())),
                    }
                }

                break ((status, reason, headers), &buf[idx .. left]);
            }
        };

        let content_length = header.2.iter().find_map(|(k, v)| if k == "content-length" { v.parse::<usize>().ok() } else { None });

        Ok(Response::new(header.0, header.2, Body::new(conn, Some(rest.to_vec()), content_length)))
    }

    fn build_req(&self, method: Method, url: Uri, headers: Vec<(String, String)>) -> String {
        let req = url.path_and_query().map(|v|v.as_str()).unwrap_or("/");
        let mut header = format!("{} {} HTTP/1.0\r\n", method.as_str(), req);

        header.push_str("Host: ");
        header.push_str(&url.host().unwrap().to_string());
        header.push_str("\r\n");
        header.push_str("Connection: close\r\n");

        for (name, value) in &headers {
            if name.to_lowercase() == "host" {
                continue;
            }

            header.push_str(name);
            header.push_str(": ");
            header.push_str(value);
            header.push_str("\r\n");
        }

        header.push_str("\r\n");

        header
    }
}

pub struct ClientBuilder<C>
    where C: Connect<Error=io::Error>,
{
    m: PhantomData<fn(C)>
}

impl<C> ClientBuilder<C>
    where C: Connect<Error=io::Error>,
{
    pub fn build(self, connector: C) -> Client<C> {
        Client::new(connector)
    }
}