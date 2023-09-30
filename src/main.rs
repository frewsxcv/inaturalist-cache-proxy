use std::convert::Infallible;
use std::net::SocketAddr;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

const PORT: u16 = 8080;

async fn hello(request: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let client = reqwest::Client::new();
    let request = build_request(&client, "GET", "observations?fields=(species_guess:!t,user:(login:!t))");
    log::info!("Request: {:?}", &request);
    let response = client.execute(request.unwrap()).await.unwrap().text().await.unwrap();
    log::info!("Response: {:?}", response);
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::init();

    let addr = SocketAddr::from(([0, 0, 0, 0], PORT));

    // We create a TcpListener and bind it to 127.0.0.1:3000
    let listener = TcpListener::bind(addr).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = http1::Builder::new()
                // `service_fn` converts our function in a `Service`
                .serve_connection(io, service_fn(hello))
                .await
            {
                log::error!("Error serving connection: {:?}", err);
            }
        });
    }
}

fn build_url(path: &str) -> String {
    format!("https://api.inaturalist.org/v2/{path}")
}

fn build_request(client: &reqwest::Client, method: &str, path: &str) -> Result<reqwest::Request, reqwest::Error> {
    // TODO: don't hardcode the method constant below
    client
        .request(reqwest::Method::GET, build_url(path))
        .header("Content-Type", "application/json")
        .build()
}
