#![feature(await_macro, async_await, futures_api, pin, arbitrary_self_types)]

use simple_http::{Request, Method, Client, HttpsConnector};
use futures::stream::StreamExt;

fn main() {
    tokio::run_async(async move {
        let connector = HttpsConnector::new(4).unwrap();
        let client = Client::new(connector);

        let url = "https://www.rust-lang.org/ru-RU/contribute.html".parse().unwrap();
        let req: Request<futures::stream::Empty<_>>= Request::new(Method::Get, url);


        let res = await!(client.request(req)).unwrap();

        println!("code {}", res.code());

        let mut body = res.into_body();
        let mut b = Vec::new();

        while let Some(chunk) = await!(body.next()) {
            b.extend_from_slice(&(chunk.unwrap())[..]);
        }

        println!("{}", String::from_utf8_lossy(&b[..]))
    });
}