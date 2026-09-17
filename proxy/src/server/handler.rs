use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response};

/// Default HTTP request handler for the proxy.
pub async fn proxy(
    _req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    Ok(Response::new(Full::new(Bytes::from("Hello from Bridge"))))
}
