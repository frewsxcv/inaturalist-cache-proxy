use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Mutex;
use tokio::net::TcpListener;

const PORT: u16 = 8080;

#[derive(Debug, Hash, Eq, PartialEq)]
struct CachedRequest {
    method: reqwest::Method,
    path: String,
}

impl CachedRequest {
    fn from_http_request(request: &HttpRequest) -> CachedRequest {
        CachedRequest {
            method: request.method().clone(),
            path: request.uri().path_and_query().unwrap().as_str().to_string(),
        }
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
struct CachedResponse {
    status: reqwest::StatusCode,
    body: Bytes,
}

impl CachedResponse {
    fn to_hyper_response(&self) -> HttpResponse {
        Response::builder()
            .status(self.status)
            .body(Full::new(self.body.clone()))
            .unwrap()
    }
}

type HttpRequest = Request<hyper::body::Incoming>;

type HttpResponseBytes = Full<Bytes>;
type HttpResponse = Response<HttpResponseBytes>;

static RESPONSE_CACHE: Lazy<Mutex<HashMap<CachedRequest, CachedResponse>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

async fn hello(request: HttpRequest) -> Result<HttpResponse, Infallible> {
    let client = reqwest::Client::new();
    let inaturalist_request_data = CachedRequest::from_http_request(&request);
    if let Some(n) = RESPONSE_CACHE
        .lock()
        .unwrap()
        .get(&inaturalist_request_data)
    {
        return Ok(n.to_hyper_response());
    }
    let request = build_request(
        &client,
        request.method().clone(),
        request.uri().path_and_query().unwrap().as_str(),
    )
    .unwrap();
    let (bytes, response) = make_request(&client, request).await;
    RESPONSE_CACHE.lock().unwrap().insert(
        inaturalist_request_data,
        CachedResponse {
            status: response.status(),
            body: bytes,
        },
    );
    Ok(response)
}

async fn make_request(
    client: &reqwest::Client,
    request: reqwest::Request,
) -> (Bytes, HttpResponse) {
    let response = client.execute(request).await.unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    (
        bytes.clone(),
        Response::builder()
            .status(status)
            .body(Full::new(bytes))
            .unwrap(),
    )
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
    format!("https://api.inaturalist.org/v2{path}")
}

fn build_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    path: &str,
) -> Result<reqwest::Request, reqwest::Error> {
    client
        .request(method, build_url(path))
        .header("Content-Type", "application/json")
        .build()
}
