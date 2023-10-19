use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

const PORT: u16 = 8080;

#[derive(Debug, Hash, Eq, PartialEq)]
struct CachedRequest {
    method: reqwest::Method,
    path: String,
}

/// Error that indicates the path and query weren't specified
#[derive(Debug)]
struct PathAndQueryNotSpecified;

impl std::fmt::Display for PathAndQueryNotSpecified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "URI path and query not specified")
    }
}

impl std::error::Error for PathAndQueryNotSpecified {}

impl CachedRequest {
    fn from_http_request(request: &HttpRequest) -> Result<CachedRequest, PathAndQueryNotSpecified> {
        let path_and_query = request
            .uri()
            .path_and_query()
            .ok_or(PathAndQueryNotSpecified)?;
        Ok(CachedRequest {
            method: request.method().clone(),
            path: path_and_query.as_str().to_string(),
        })
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
struct CachedResponse {
    /// Timestamp for when the cache entry expires
    expires_on: Instant,
    status: reqwest::StatusCode,
    body: Bytes,
}

impl CachedResponse {
    fn to_hyper_response(&self) -> HttpResponse {
        Response::builder()
            .status(self.status)
            .header("Access-Control-Allow-Origin", "*")
            .body(Full::new(self.body.clone()))
            .unwrap()
    }
}

type HttpRequest = Request<hyper::body::Incoming>;

type HttpResponseBytes = Full<Bytes>;
type HttpResponse = Response<HttpResponseBytes>;

static RESPONSE_CACHE: Lazy<Mutex<HashMap<CachedRequest, CachedResponse>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static INATURALIST_RATE_LIMIT_AMOUNT: Lazy<governor::Quota> =
    Lazy::new(|| governor::Quota::with_period(std::time::Duration::from_secs(2)).unwrap());

static INATURALIST_RATE_LIMITER: Lazy<
    governor::RateLimiter<
        governor::state::direct::NotKeyed,
        governor::state::InMemoryState,
        governor::clock::DefaultClock,
    >,
> = Lazy::new(|| governor::RateLimiter::direct(*INATURALIST_RATE_LIMIT_AMOUNT));

// Length in seconds to cache a response for
static CACHE_TTL_REQUEST_HEADER: &str = "X-CACHE-TTL";

async fn hello(request: HttpRequest) -> Result<HttpResponse, PathAndQueryNotSpecified> {
    println!("Received request: {:?}", request);
    // If is option request respond with Access-Control-Allow-Headers
    if request.method() == hyper::Method::OPTIONS {
        return Ok(Response::builder()
            .status(200)
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Headers", CACHE_TTL_REQUEST_HEADER)
            .body(Full::new(Bytes::new()))
            .unwrap());
    }

    let client = reqwest::Client::new();
    let cache_ttl_duration = Duration::from_secs(
        request
            .headers()
            .get(CACHE_TTL_REQUEST_HEADER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60 * 60 * 24 * 7),
    ); // Default to 7 days
    let inaturalist_request_data = CachedRequest::from_http_request(&request)?;
    if let Some(n) = RESPONSE_CACHE
        .lock()
        .unwrap()
        .get(&inaturalist_request_data)
    {
        if Instant::now() < n.expires_on {
            return Ok(n.to_hyper_response());
        } else {
            RESPONSE_CACHE.lock().unwrap().remove(&inaturalist_request_data);
        }
    }
    let request = build_request(
        &client,
        request.method().clone(),
        request.uri().path_and_query().unwrap().as_str(),
    )
    .unwrap();
    let (bytes, response) = make_request(&client, request).await;
    if response.status().is_success() {
        RESPONSE_CACHE.lock().unwrap().insert(
            inaturalist_request_data,
            CachedResponse {
                expires_on: (Instant::now() + cache_ttl_duration),
                status: response.status(),
                body: bytes,
            },
        );
    }
    Ok(response)
}

async fn make_request(
    client: &reqwest::Client,
    request: reqwest::Request,
) -> (Bytes, HttpResponse) {
    INATURALIST_RATE_LIMITER.until_ready().await;
    let response = client.execute(request).await.unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    (
        bytes.clone(),
        Response::builder()
            .status(status)
            .header("Access-Control-Allow-Origin", "*")
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
    format!("https://api.inaturalist.org/v1{path}")
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
