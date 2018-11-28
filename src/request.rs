use futures::stream::{self, Stream};
use hyper::Uri;
use std::io;
use std::marker::PhantomData;
use http::{
    Version,
    Method,
};


pub struct Request<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub(crate) method: Method,
    pub(crate) version: Version,
    pub(crate) uri: Uri,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Option<B>,
}

impl<B> Request<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub fn new(method: Method, uri: Uri) -> Request<B> {
        Request {
            method,
            uri,
            version: Version::HTTP_10,
            headers: vec![
                ("User-Agent".to_string(), "Simple http request".to_string())
            ],
            body: None
        }
    }

    pub fn builder() -> RequestBuilder<B> {
        RequestBuilder::default()
    }
}

pub struct RequestBuilder<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub(crate) method: Method,
    pub(crate) version: Version,
    pub(crate) uri: Uri,
    pub(crate) headers: Vec<(String, String)>,
    _m: PhantomData<B>,
}

impl <B> Default for RequestBuilder<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    fn default() -> Self {
        RequestBuilder {
            method: Default::default(),
            version: Default::default(),
            uri: Default::default(),
            headers: Default::default(),
            _m: Default::default(),
        }
    }
}

impl RequestBuilder<stream::Empty<io::Result<Vec<u8>>>> {
    pub fn done(self) -> Result<Request<stream::Empty<io::Result<Vec<u8>>>>, io::Error> {
        Ok(Request {
            method: self.method,
            version: self.version,
            uri: self.uri,
            headers: self.headers,
            body: None,
        })
    }
}

impl <B> RequestBuilder<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub fn version(self, version: Version) -> Self {
        Self {
            version,
            .. self
        }
    }

    pub fn uri(self, uri: Uri) -> Self {
        Self {
            uri,
            .. self
        }
    }

    pub fn header(self, name: &str, value: &str) -> Self {
        let RequestBuilder {
            method,
            version,
            uri,
            mut headers,
            _m
        } = self;

        headers.push((name.to_string(), value.to_string()));

        Self {
            method,
            version,
            uri,
            headers,
            _m
        }
    }

    pub fn body(self, body: B) -> Result<Request<B>, io::Error> {
        Ok(Request {
            method: self.method,
            version: self.version,
            uri: self.uri,
            headers: self.headers,
            body: Some(body),
        })
    }
}
