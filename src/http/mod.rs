pub mod dns;
pub mod ares;

use std::borrow::Cow;
use std::fmt;
use std::error::Error as StdError;
use std::io;
use std::mem;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use futures_legacy::{try_ready, Async, Future, Poll};
use futures_legacy::future::{Executor};
use http::uri::Scheme;
use net2::TcpBuilder;
use tokio_reactor::Handle;
use tokio_tcp::{TcpStream, ConnectFuture};
use tokio_timer::Delay;

use crate::connect::{Connect, Connected, Destination};
use self::dns::{GaiResolver, Resolve, TokioThreadpoolGaiResolver};
use self::ares::CAresResolverImpl;
use std::sync::Arc;

#[derive(Clone)]
pub struct HttpConnector<R = Arc<CAresResolverImpl>> {
    enforce_http: bool,
    handle: Option<Handle>,
    happy_eyeballs_timeout: Option<Duration>,
    keep_alive_timeout: Option<Duration>,
    local_address: Option<IpAddr>,
    nodelay: bool,
    resolver: R,
    reuse_address: bool,
}

impl HttpConnector {
    /// Construct a new HttpConnector.
    ///
    /// Takes number of DNS worker threads.
    #[inline]
    pub fn new(threads: usize) -> HttpConnector {
        HttpConnector::new_with_resolver(Arc::new(CAresResolverImpl::new()))
    }
}

impl<R> HttpConnector<R> {

    /// Construct a new HttpConnector.
    ///
    /// Takes a `Resolve` to handle DNS lookups.
    pub fn new_with_resolver(resolver: R) -> HttpConnector<R> {
        HttpConnector {
            enforce_http: true,
            handle: None,
            happy_eyeballs_timeout: Some(Duration::from_millis(300)),
            keep_alive_timeout: None,
            local_address: None,
            nodelay: false,
            resolver,
            reuse_address: false,
        }
    }

    #[inline]
    pub fn enforce_http(&mut self, is_enforced: bool) {
        self.enforce_http = is_enforced;
    }
}

impl<R> Connect for HttpConnector<R>
    where
        R: Resolve + Clone + Send + Sync,
        R::Future: Send,
{
    type Transport = TcpStream;
    type Error = io::Error;
    type Future = HttpConnecting<R>;

    fn connect(&self, dst: Destination) -> Self::Future {
        if self.enforce_http {
            if dst.uri.scheme_part() != Some(&Scheme::HTTP) {
                return invalid_url(InvalidUrl::NotHttp, &self.handle);
            }
        } else if dst.uri.scheme_part().is_none() {
            return invalid_url(InvalidUrl::MissingScheme, &self.handle);
        }

        let host = match dst.uri.host() {
            Some(s) => s,
            None => return invalid_url(InvalidUrl::MissingAuthority, &self.handle),
        };
        let port = match dst.uri.port() {
            Some(port) => port,
            None => if dst.uri.scheme_part() == Some(&Scheme::HTTPS) { 443 } else { 80 },
        };

        HttpConnecting {
            state: State::Lazy(self.resolver.clone(), host.into(), self.local_address),
            handle: self.handle.clone(),
            happy_eyeballs_timeout: self.happy_eyeballs_timeout,
            keep_alive_timeout: self.keep_alive_timeout,
            nodelay: self.nodelay,
            port,
            reuse_address: self.reuse_address,
        }
    }
}


#[derive(Debug, Clone, Copy)]
enum InvalidUrl {
    MissingScheme,
    NotHttp,
    MissingAuthority,
}

impl fmt::Display for InvalidUrl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.description())
    }
}

impl StdError for InvalidUrl {
    fn description(&self) -> &str {
        match *self {
            InvalidUrl::MissingScheme => "invalid URL, missing scheme",
            InvalidUrl::NotHttp => "invalid URL, scheme must be http",
            InvalidUrl::MissingAuthority => "invalid URL, missing domain",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpInfo {
    remote_addr: SocketAddr,
}


impl HttpInfo {
    /// Get the remote address of the transport used.
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

#[inline]
fn invalid_url<R: Resolve>(err: InvalidUrl, handle: &Option<Handle>) -> HttpConnecting<R> {
    HttpConnecting {
        state: State::Error(Some(io::Error::new(io::ErrorKind::InvalidInput, err))),
        handle: handle.clone(),
        keep_alive_timeout: None,
        nodelay: false,
        port: 0,
        happy_eyeballs_timeout: None,
        reuse_address: false,
    }
}

#[must_use = "futures do nothing unless polled"]
pub struct HttpConnecting<R: Resolve = GaiResolver> {
    state: State<R>,
    handle: Option<Handle>,
    happy_eyeballs_timeout: Option<Duration>,
    keep_alive_timeout: Option<Duration>,
    nodelay: bool,
    port: u16,
    reuse_address: bool,
}

enum State<R: Resolve> {
    Lazy(R, String, Option<IpAddr>),
    Resolving(R::Future, Option<IpAddr>),
    Connecting(ConnectingTcp),
    Error(Option<io::Error>),
}

impl<R: Resolve> Future for HttpConnecting<R> {
    type Item = (TcpStream, Connected);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let state;
            match self.state {
                State::Lazy(ref resolver, ref mut host, local_addr) => {
                    // If the host is already an IP addr (v4 or v6),
                    // skip resolving the dns and start connecting right away.
                    if let Some(addrs) = dns::IpAddrs::try_parse(host, self.port) {
                        state = State::Connecting(ConnectingTcp::new(
                            local_addr, addrs, self.happy_eyeballs_timeout, self.reuse_address));
                    } else {
                        let name = dns::Name::new(mem::replace(host, String::new()));
                        state = State::Resolving(resolver.resolve(name), local_addr);
                    }
                },
                State::Resolving(ref mut future, local_addr) => {
                    match future.poll()? {
                        Async::NotReady => return Ok(Async::NotReady),
                        Async::Ready(addrs) => {
                            let port = self.port;
                            let addrs = addrs
                                .map(|addr| SocketAddr::new(addr, port))
                                .collect();
                            let addrs = dns::IpAddrs::new(addrs);
                            state = State::Connecting(ConnectingTcp::new(
                                local_addr, addrs, self.happy_eyeballs_timeout, self.reuse_address));
                        }
                    };
                },
                State::Connecting(ref mut c) => {
                    let sock = try_ready!(c.poll(&self.handle));

                    if let Some(dur) = self.keep_alive_timeout {
                        sock.set_keepalive(Some(dur))?;
                    }

                    sock.set_nodelay(self.nodelay)?;

                    let extra = HttpInfo {
                        remote_addr: sock.peer_addr()?,
                    };
                    let connected = Connected::new()
                        .extra(extra);

                    return Ok(Async::Ready((sock, connected)));
                },
                State::Error(ref mut e) => return Err(e.take().expect("polled more than once")),
            }
            self.state = state;
        }
    }
}

impl<R: Resolve + fmt::Debug> fmt::Debug for HttpConnecting<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad("HttpConnecting")
    }
}

struct ConnectingTcp {
    local_addr: Option<IpAddr>,
    preferred: ConnectingTcpRemote,
    fallback: Option<ConnectingTcpFallback>,
    reuse_address: bool,
}

impl ConnectingTcp {
    fn new(
        local_addr: Option<IpAddr>,
        remote_addrs: dns::IpAddrs,
        fallback_timeout: Option<Duration>,
        reuse_address: bool,
    ) -> ConnectingTcp {
        if let Some(fallback_timeout) = fallback_timeout {
            let (preferred_addrs, fallback_addrs) = remote_addrs.split_by_preference();
            if fallback_addrs.is_empty() {
                return ConnectingTcp {
                    local_addr,
                    preferred: ConnectingTcpRemote::new(preferred_addrs),
                    fallback: None,
                    reuse_address,
                };
            }

            ConnectingTcp {
                local_addr,
                preferred: ConnectingTcpRemote::new(preferred_addrs),
                fallback: Some(ConnectingTcpFallback {
                    delay: Delay::new(Instant::now() + fallback_timeout),
                    remote: ConnectingTcpRemote::new(fallback_addrs),
                }),
                reuse_address,
            }
        } else {
            ConnectingTcp {
                local_addr,
                preferred: ConnectingTcpRemote::new(remote_addrs),
                fallback: None,
                reuse_address,
            }
        }
    }
}

struct ConnectingTcpFallback {
    delay: Delay,
    remote: ConnectingTcpRemote,
}

struct ConnectingTcpRemote {
    addrs: dns::IpAddrs,
    current: Option<ConnectFuture>,
}

impl ConnectingTcpRemote {
    fn new(addrs: dns::IpAddrs) -> Self {
        Self {
            addrs,
            current: None,
        }
    }
}

impl ConnectingTcpRemote {
    // not a Future, since passing a &Handle to poll
    fn poll(
        &mut self,
        local_addr: &Option<IpAddr>,
        handle: &Option<Handle>,
        reuse_address: bool,
    ) -> Poll<TcpStream, io::Error> {
        let mut err = None;
        loop {
            if let Some(ref mut current) = self.current {
                match current.poll() {
                    Ok(ok) => return Ok(ok),
                    Err(e) => {
                        err = Some(e);
                        if let Some(addr) = self.addrs.next() {
                            *current = connect(&addr, local_addr, handle, reuse_address)?;
                            continue;
                        }
                    }
                }
            } else if let Some(addr) = self.addrs.next() {
                self.current = Some(connect(&addr, local_addr, handle, reuse_address)?);
                continue;
            }

            return Err(err.take().expect("missing connect error"));
        }
    }
}

fn connect(addr: &SocketAddr, local_addr: &Option<IpAddr>, handle: &Option<Handle>, reuse_address: bool) -> io::Result<ConnectFuture> {
    let builder = match addr {
        &SocketAddr::V4(_) => TcpBuilder::new_v4()?,
        &SocketAddr::V6(_) => TcpBuilder::new_v6()?,
    };

    if reuse_address {
        builder.reuse_address(reuse_address)?;
    }

    if let Some(ref local_addr) = *local_addr {
        // Caller has requested this socket be bound before calling connect
        builder.bind(SocketAddr::new(local_addr.clone(), 0))?;
    }
        else if cfg!(windows) {
            // Windows requires a socket be bound before calling connect
            let any: SocketAddr = match addr {
                &SocketAddr::V4(_) => {
                    ([0, 0, 0, 0], 0).into()
                },
                &SocketAddr::V6(_) => {
                    ([0, 0, 0, 0, 0, 0, 0, 0], 0).into()
                }
            };
            builder.bind(any)?;
        }

    let handle = match *handle {
        Some(ref handle) => Cow::Borrowed(handle),
        None => Cow::Owned(Handle::current()),
    };

    Ok(TcpStream::connect_std(builder.to_tcp_stream()?, addr, &handle))
}

impl ConnectingTcp {
    // not a Future, since passing a &Handle to poll
    fn poll(&mut self, handle: &Option<Handle>) -> Poll<TcpStream, io::Error> {
        match self.fallback.take() {
            None => self.preferred.poll(&self.local_addr, handle, self.reuse_address),
            Some(mut fallback) => match self.preferred.poll(&self.local_addr, handle, self.reuse_address) {
                Ok(Async::Ready(stream)) => {
                    // Preferred successful - drop fallback.
                    Ok(Async::Ready(stream))
                }
                Ok(Async::NotReady) => match fallback.delay.poll() {
                    Ok(Async::Ready(_)) => match fallback.remote.poll(&self.local_addr, handle, self.reuse_address) {
                        Ok(Async::Ready(stream)) => {
                            // Fallback successful - drop current preferred,
                            // but keep fallback as new preferred.
                            self.preferred = fallback.remote;
                            Ok(Async::Ready(stream))
                        }
                        Ok(Async::NotReady) => {
                            // Neither preferred nor fallback are ready.
                            self.fallback = Some(fallback);
                            Ok(Async::NotReady)
                        }
                        Err(_) => {
                            // Fallback failed - resume with preferred only.
                            Ok(Async::NotReady)
                        }
                    },
                    Ok(Async::NotReady) => {
                        // Too early to attempt fallback.
                        self.fallback = Some(fallback);
                        Ok(Async::NotReady)
                    }
                    Err(_) => {
                        // Fallback delay failed - resume with preferred only.
                        Ok(Async::NotReady)
                    }
                }
                Err(_) => {
                    // Preferred failed - use fallback as new preferred.
                    self.preferred = fallback.remote;
                    self.preferred.poll(&self.local_addr, handle, self.reuse_address)
                }
            }
        }
    }
}
