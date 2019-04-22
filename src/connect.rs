use std::error::Error as StdError;
use std::{fmt, mem};

use bytes::{BufMut, Bytes, BytesMut};
use futures_legacy::future::Future;
use http::{uri, Uri};
use tokio_io::{AsyncRead, AsyncWrite};
pub use crate::http::HttpConnector;
pub use hyper::client::connect::{Connected};

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Parse
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl StdError for Error {}


pub trait Connect: Send + Sync {
    /// The connected IO Stream.
    type Transport: AsyncRead + AsyncWrite + Send + 'static;
    /// An error occured when trying to connect.
    type Error: Into<Box<StdError + Send + Sync>>;
    /// A Future that will resolve to the connected Transport.
    type Future: Future<Item=(Self::Transport, Connected), Error=Self::Error> + Send;
    /// Connect to a destination.
    fn connect(&self, dst: Destination) -> Self::Future;
}


#[derive(Clone, Debug)]
pub struct Destination {
    pub(super) uri: Uri,
}

impl Destination {
    /// Get the protocol scheme.
    #[inline]
    pub fn scheme(&self) -> &str {
        self.uri
            .scheme_part()
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Get the hostname.
    #[inline]
    pub fn host(&self) -> &str {
        self.uri
            .host()
            .unwrap_or("")
    }

    /// Get the port, if specified.
    #[inline]
    pub fn port(&self) -> Option<u16> {
        self.uri.port_u16()
    }

    /// Update the scheme of this destination.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use hyper::client::connect::Destination;
    /// # fn with_dst(mut dst: Destination) {
    /// // let mut dst = some_destination...
    /// // Change from "http://"...
    /// assert_eq!(dst.scheme(), "http");
    ///
    /// // to "ws://"...
    /// dst.set_scheme("ws");
    /// assert_eq!(dst.scheme(), "ws");
    /// # }
    /// ```
    ///
    /// # Error
    ///
    /// Returns an error if the string is not a valid scheme.
    pub fn set_scheme(&mut self, scheme: &str) -> Result<()> {
        let scheme = scheme.parse().map_err(|_| Error::Parse)?;
        self.update_uri(move |parts| {
            parts.scheme = Some(scheme);
        })
    }

    /// Update the host of this destination.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use hyper::client::connect::Destination;
    /// # fn with_dst(mut dst: Destination) {
    /// // let mut dst = some_destination...
    /// // Change from "hyper.rs"...
    /// assert_eq!(dst.host(), "hyper.rs");
    ///
    /// // to "some.proxy"...
    /// dst.set_host("some.proxy");
    /// assert_eq!(dst.host(), "some.proxy");
    /// # }
    /// ```
    ///
    /// # Error
    ///
    /// Returns an error if the string is not a valid hostname.
    pub fn set_host(&mut self, host: &str) -> Result<()> {
        // Prevent any userinfo setting, it's bad!
        if host.contains('@') {
            return Err(Error::Parse);
        }
        let auth = if let Some(port) = self.port() {
            let bytes = Bytes::from(format!("{}:{}", host, port));
            uri::Authority::from_shared(bytes)
                .map_err(|_| Error::Parse)?
        } else {
            let auth = host.parse::<uri::Authority>().map_err(|_| Error::Parse)?;
            if auth.port_u16().is_some() {
                return Err(Error::Parse);
            }
            auth
        };
        self.update_uri(move |parts| {
            parts.authority = Some(auth);
        })
    }

    /// Update the port of this destination.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use hyper::client::connect::Destination;
    /// # fn with_dst(mut dst: Destination) {
    /// // let mut dst = some_destination...
    /// // Change from "None"...
    /// assert_eq!(dst.port(), None);
    ///
    /// // to "4321"...
    /// dst.set_port(4321);
    /// assert_eq!(dst.port(), Some(4321));
    ///
    /// // Or remove the port...
    /// dst.set_port(None);
    /// assert_eq!(dst.port(), None);
    /// # }
    /// ```
    pub fn set_port<P>(&mut self, port: P)
        where
            P: Into<Option<u16>>,
    {
        self.set_port_opt(port.into());
    }

    fn set_port_opt(&mut self, port: Option<u16>) {
        use std::fmt::Write;

        let auth = if let Some(port) = port {
            let host = self.host();
            // Need space to copy the hostname, plus ':',
            // plus max 5 port digits...
            let cap = host.len() + 1 + 5;
            let mut buf = BytesMut::with_capacity(cap);
            buf.put_slice(host.as_bytes());
            buf.put_u8(b':');
            write!(buf, "{}", port)
                .expect("should have space for 5 digits");

            uri::Authority::from_shared(buf.freeze())
                .expect("valid host + :port should be valid authority")
        } else {
            self.host().parse()
                .expect("valid host without port should be valid authority")
        };

        self.update_uri(move |parts| {
            parts.authority = Some(auth);
        })
            .expect("valid uri should be valid with port");
    }

    fn update_uri<F>(&mut self, f: F) -> Result<()>
        where
            F: FnOnce(&mut uri::Parts)
    {
        // Need to store a default Uri while we modify the current one...
        let old_uri = mem::replace(&mut self.uri, Uri::default());
        // However, mutate a clone, so we can revert if there's an error...
        let mut parts: uri::Parts = old_uri.clone().into();

        f(&mut parts);

        match Uri::from_parts(parts) {
            Ok(uri) => {
                self.uri = uri;
                Ok(())
            },
            Err(_err) => {
                self.uri = old_uri;
                Err(Error::Parse)
            },
        }
    }

    /*
    /// Returns whether this connection must negotiate HTTP/2 via ALPN.
    pub fn must_h2(&self) -> bool {
        match self.alpn {
            Alpn::Http1 => false,
            Alpn::H2 => true,
        }
    }
    */
}