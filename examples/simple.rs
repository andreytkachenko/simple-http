use simple_http::{Request, Method, Client, HttpsConnector};
use futures::stream::StreamExt;
use futures::executor;

fn main() {
    executor::block_on(async move {
        let connector = HttpsConnector::new(4).unwrap();
        let client = Client::new(connector);

        let url = "https://www.vrbo.com/781849".parse().unwrap();
        let req: Request<futures::stream::Empty<_>>= Request::new(Method::GET, url);


        let res = client.request(req).await.unwrap();

        println!("code {}", res.status());

        let mut body = res.into_body();
        let mut b = Vec::new();

        while let Some(chunk) = body.next().await {
            b.extend_from_slice(&(chunk.unwrap())[..]);
        }

        println!("{:?}", String::from_utf8_lossy(&b[..]))
    });
}