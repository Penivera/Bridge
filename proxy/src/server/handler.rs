#![allow(warnings)]
use super::Proxy;
use bytes::Bytes;
use hyper::{Method, Request, Response, StatusCode, body::Incoming as IncomingBody, header::HOST, service::Service, upgrade::Upgraded};
use hyper_util::rt::TokioIo;
use registry::Route;
use tokio::net::TcpStream;

use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};

use std::{
    convert::Infallible, future::{Ready, ready}, pin::Pin,
};
use futures::FutureExt;



impl Service<Request<IncomingBody>> for Proxy {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<IncomingBody>) -> Self::Future {
        let host = match req.headers().get(HOST).and_then( |header| header.to_str().ok()) {
            Some(host) => host.to_owned(),
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
            let upstream = this.map_url_to_route(&host).await;
            match upstream {
                Some(upstream) => {
                    let Some(addr) = upstream.upstream else {
                        return Ok(Response::new(Full::new(Bytes::from(format!(
                            "No upstream found for host {host}"
                        ))).map_err(|never| match never {}).boxed()));
                    };
                    this.handle_request(req, addr.to_string()).await
                },
                None => Ok(Response::new(Full::new(Bytes::from(format!(
                    "No route found for host {host}"
                ))).map_err(|never| match never {}).boxed())),
            }
        }
        .boxed()
    }
}

impl Proxy {
   async fn map_url_to_route(&self, host: &str) -> Option<Route> {
        let route = self.registry.lookup(host);
        route
    }

    async fn tunnel(&self, upgraded:Upgraded, addr: String) -> std::io::Result<()>{
        // Connect to the upstream server
        let mut upstream = TcpStream::connect(addr).await?;
        let mut upgraded = TokioIo::new(upgraded);

        // Proxying
        let (client_stream,server_stream) = tokio::io::copy_bidirectional(&mut upgraded, &mut upstream).await?;
        tracing::debug!("Client stream: {client_stream:?}, server stream: {server_stream:?}");
        Ok(())
    }

    async fn handle_request(&self, request: Request<IncomingBody>, upstream: String) -> Result<Response<BoxBody<Bytes,hyper::Error>>,hyper::Error> {
        if Method::CONNECT == request.method() {
            // Return an empty body to conncet request then upgrade the connection
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

            let stream = match TcpStream::connect((host, port)).await {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::error!("Connection error: {e:?}");

                    let body = Full::new(Bytes::from("Upstream connection failed"))
                        .map_err(|never: Infallible| match never {})
                        .boxed();
                    
                    let mut response = Response::new(body);
            
                    *response.status_mut() = StatusCode::BAD_GATEWAY;
            
                    return Ok(response);
                }
            };
            let io = TokioIo::new(stream);

            let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
                .await?;
            tokio::spawn(
                async move {
                    if let Err(err) = conn.await {
                        tracing::error!("Handshake error: {err:?}");
                    }
                }
            );
            let resp = sender.send_request(request).await?;
            Ok(resp.map( |b| b.boxed()))
            
        }
    }
}


