use axum::{body::Body, extract::Request, http::header, response::Response};
use rust_embed::RustEmbed;
use std::{convert::Infallible, future::Future, pin::Pin};
use tower::Service;

#[derive(RustEmbed)]
#[folder = "../static/"]
#[prefix = "/"]
pub struct Assets;

#[derive(Default, Clone)]
pub struct AssetsService<S = DefaultServeDirFallback> {
    inner: Option<S>,
}

impl AssetsService<DefaultServeDirFallback> {
    pub fn new() -> Self {
        Self { inner: None }
    }
}

impl<S> Service<Request> for AssetsService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match &mut self.inner {
            Some(inner) => inner.poll_ready(cx),
            _ => std::task::Poll::Ready(Ok(())),
        }
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let uri = req.uri().clone();

        Box::pin(async move {
            let resp = match Assets::get(uri.path()) {
                Some(content) => {
                    let mime = mime_guess::from_path(uri.path()).first_or_octet_stream();

                    Response::builder()
                        .header(header::CONTENT_TYPE, mime.as_ref())
                        .body(Body::from(content.data))
                        .unwrap()
                }
                _ => Response::builder()
                    .status(404)
                    .body(Body::from("404 Not Found"))
                    .unwrap(),
            };

            Ok(resp)
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DefaultServeDirFallback(Infallible);

impl Service<Request> for DefaultServeDirFallback {
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.0 {}
    }

    fn call(&mut self, _req: Request) -> Self::Future {
        match self.0 {}
    }
}
