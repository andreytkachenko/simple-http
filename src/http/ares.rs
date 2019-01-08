use c_ares_resolver::{FutureResolver, CAresFuture};
use super::dns::{Resolve, Name};
use std::net::IpAddr;
use std::vec::IntoIter;
use std::io::{Error, ErrorKind};
use std::mem;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;
use futures_legacy::{Poll, Async, Future as LegacyFuture};

pub enum CAresResolverFuture {
    FromResolver(CAresFuture<c_ares::AResults>, String, ResolverCache),
    FromCache(Vec<IpAddr>),
    Done,
}

impl LegacyFuture for CAresResolverFuture {
    type Item = IntoIter<IpAddr>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let state = match mem::replace(self, CAresResolverFuture::Done) {
            CAresResolverFuture::FromResolver(mut fut, name, cache) => {
                match fut.poll() {
                    Ok(Async::Ready(vals)) => {
                        let items: Vec<_> = vals.into_iter().map(|res| IpAddr::V4(res.ipv4())).collect();

                        cache.add(name, items.clone());

                        return Ok(Async::Ready(items.into_iter()));
                    },

                    Err(err) => {
                        return Err(Error::new(ErrorKind::Other, Box::new(err)))
                    },

                    _ => CAresResolverFuture::FromResolver(fut, name, cache),
                }
            },

            CAresResolverFuture::FromCache(vec) => {
                return Ok(Async::Ready(vec.into_iter()))
            },

            CAresResolverFuture::Done => {
                panic!("cannot poll resolved future");
            }
        };

        mem::replace(self, state);

        Ok(Async::NotReady)
    }
}

#[derive(Clone, Default)]
pub struct ResolverCache {
    cache: Arc<Mutex<HashMap<String, (Vec<IpAddr>, Instant)>>>,
}

impl ResolverCache {
    pub fn lookup(&self, name: String) -> Option<Vec<IpAddr>> {
        let lock = self.cache.lock().ok()?;
        let (vec, ttl) = lock.get(&name)?;

        if ttl < &Instant::now() {
            Some(vec.clone())
        } else {
            None
        }
    } 

    pub fn add(&self, name: String, addrs: Vec<IpAddr>) {
        let mut lock = self.cache.lock().unwrap();

        lock.insert(name, (addrs, Instant::now()));
    }
}

pub type CAresResolver = Arc<CAresResolverImpl>;

pub struct CAresResolverImpl {
    cache: ResolverCache,
    resolver: FutureResolver,
}

impl CAresResolverImpl {
    pub fn new() -> Self {
        CAresResolverImpl {
            resolver: FutureResolver::new().unwrap(),
            cache: Default::default(),
        }
    }
}

impl Resolve for CAresResolverImpl {
    type Addrs = IntoIter<IpAddr>;
    type Future = CAresResolverFuture;

    fn resolve(&self, name: Name) -> Self::Future {
        if let Some(values) = self.cache.lookup(name.as_str().to_string()) {
            return CAresResolverFuture::FromCache(values);
        }

        CAresResolverFuture::FromResolver(
            self.resolver.query_a(name.as_str()), 
            name.as_str().to_string(), 
            self.cache.clone()
        )
    }
}