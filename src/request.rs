use futures::stream::Stream;
use crate::Method;
use hyper::Uri;
use std::io;


pub struct Request<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub(crate) method: Method,
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
            headers: vec![
                ("User-Agent".to_string(), "Simple http request".to_string())
            ],
            body: None
        }
    }
}
