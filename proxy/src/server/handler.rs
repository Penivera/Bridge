#![allow(warnings)]
use super::Proxy;
use bytes::Bytes;
use hyper::{Method, Request, Response, StatusCode, body::{Incoming as IncomingBody,}, header::HOST, service::Service, upgrade::Upgraded};
use hyper_util::rt::TokioIo;
use registry::Route;
use tokio::net::TcpStream;

use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};

use std::{
    convert::Infallible, fmt::format, future::{Ready, ready}, pin::Pin,
};
use futures::FutureExt;

/// Strips a trailing `:<port>` from a Host header value (clients include the
/// port for non-default ports) and lowercases it for registry lookup.
/// Handles bracketed IPv6 literals too.
pub fn normalize_host(host: &str) -> String {
    let bare = if host.starts_with('[') {
        match host.find(']') {
            Some(end) => &host[1..end],
            None => host,
        }
    } else if let Some(colon) = host.find(':') {
        &host[..colon]
    } else {
        host
    };
    bare.trim_end_matches('.').to_ascii_lowercase()
}



impl Service<Request<IncomingBody>> for Proxy {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<IncomingBody>) -> Self::Future {
        let host = match req.headers().get(HOST).and_then( |header| header.to_str().ok()) {
            Some(host) => normalize_host(host),
            None => {
                return async {
                    Ok(Response::new(Full::new(Bytes::from(
                        "No host found in request uri",
                    )).map_err(|never| match never {}).boxed()))
                }
                .boxed();
            }
        };
        let this = self.clone();
        async move {
            let started = std::time::Instant::now();
            let upstream = this.map_url_to_route(&host).await;
            match upstream {
                Some(upstream) => {
                    let Some(addr) = upstream.upstream else {
                        this.registry.record_request(&host, started.elapsed().as_millis() as u64, true);
                        return Ok(Response::new(Full::new(Bytes::from(format!(
                            "No upstream found for host {host}"
                        ))).map_err(|never| match never {}).boxed()));
                    };
                    let result = this.handle_request(req, addr.to_string()).await;
                    let is_error = match &result {
                        Ok(resp) => resp.status().is_server_error(),
                        Err(_) => true,
                    };
                    this.registry.record_request(&host, started.elapsed().as_millis() as u64, is_error);
                    result
                },
                None => {
                    this.registry.record_request(&host, started.elapsed().as_millis() as u64, true);
                    Ok(Response::new(Full::new(Bytes::from(format!(
                        "No route found for host {host}"
                    ))).map_err(|never| match never {}).boxed()))
                },
            }
        }
        .boxed()
    }
}

impl Proxy {
   async fn map_url_to_route(&self, host: &str) -> Option<Route> {
        let route = self.registry.lookup(host);
        route
    }    async fn tunnel(&self, upgraded:Upgraded, addr: String) -> std::io::Result<()>{
        // Connect to the upstream server
        let mut upstream = TcpStream::connect(addr).await?;
        let mut upgraded = TokioIo::new(upgraded);

        // Proxying
        let (client_stream,server_stream) = tokio::io::copy_bidirectional(&mut upgraded, &mut upstream).await?;
        tracing::debug!("Client stream: {client_stream:?}, server stream: {server_stream:?}");
        Ok(())
    }

    async fn handle_request(&self,  mut request: Request<IncomingBody>, upstream: String) -> Result<Response<BoxBody<Bytes,hyper::Error>>,hyper::Error> {
        if Method::CONNECT == request.method() {
            // Return an empty body to connect request then upgrade the connection
            let this = self.clone();
            tokio::spawn(
                async move {
                    match hyper::upgrade::on(request).await {
                        Ok(upgraded) => {
                            if let Err(e) = this.tunnel(upgraded, upstream).await {
                               tracing::error!("Tunnel error: {e:?}");
                            }
                        }
                        Err(e) => {
                            tracing::error!("Upgrade error: {e:?}");
                        }
                    }
                }
            );
            Ok(Response::new(Empty::<Bytes>::new().map_err( | never | match never {}).boxed()))
        }
        else{
            let parts: Vec<&str> = upstream.splitn(2, ':').collect();
            let host = parts[0];
            let port: u16 = parts.get(1).and_then(|p| p.parse().ok()).unwrap();
            let path_and_query = request.uri().path_and_query().map(|pq | pq.as_str()).unwrap_or("/");

            let upstream_url = format!("http://{upstream}{path_and_query}").parse::<hyper::Uri>().unwrap();
           

            // Reusing the client which internally uses connection pooling

            *request.uri_mut() = upstream_url;
            tracing::debug!("{}",request.uri());
            let resp = match self.client.request(request.map(|b| b.boxed())).await{
                Ok(resp) => resp.map(|b| b.boxed()),
                Err(e) => {
                    tracing::error!("Request error: {e:?}");
                    let body = Full::new(Bytes::from("Upstream connection failed"))
                        .map_err(|never: Infallible| match never {})
                        .boxed();
                    let mut response = Response::new(body);
                    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
                    response
                }
            };
            Ok(resp)
        }
    }
}


