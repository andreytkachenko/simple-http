use futures::stream::Stream;
use std::io;
use std::pin::Pin;
use std::task::{Poll, LocalWaker};
use tokio::prelude::Async;
use crate::body::Body;
use ::http::StatusCode;
use std::collections::HashMap;
use std::iter::FromIterator;


pub struct HeaderMap {
    map: HashMap<String, Vec<String>>,
}

impl HeaderMap {
    pub fn new(vec: Vec<(String, String)>) -> Self {
        let mut map: HashMap<String, Vec<String>> = HashMap::with_capacity(vec.len());

        for (k, v) in vec {
            if let Some(vec) = map.get_mut(&k)  {
                vec.push(v);
            } else {
                map.insert(k, vec![v]);
            }
        }

        Self { map }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.map.get(name)
            .and_then(|val| val.get(0))
            .map(|val| &**val)
    }
}


pub struct Response<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub(crate) status_code: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: B,
}

impl<B> Response<B>
    where B: Stream<Item = io::Result<Vec<u8>>> + Send
{
    pub fn new(status_code: u16, headers: Vec<(String, String)>, body: B) -> Self {
        Self {
            status_code: StatusCode::from_u16(status_code).unwrap(),
            headers: HeaderMap::new(headers),
            body,
        }
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn status(&self) -> StatusCode {
        self.status_code
    }

    pub fn into_body(self) -> B {
        self.body
    }
}