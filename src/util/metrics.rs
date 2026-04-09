use anyhow::Result;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode};
use prometheus::{Encoder, HistogramOpts, HistogramVec, IntCounter, Registry, TextEncoder};
use std::net::SocketAddr;
use std::sync::OnceLock;

// ---- Global registry & metrics ----

static REGISTRY: OnceLock<Registry> = OnceLock::new();
static GEOCODE_SUCCESS: OnceLock<IntCounter> = OnceLock::new();
static GEOCODE_FAILURE: OnceLock<IntCounter> = OnceLock::new();
static GEOCODE_LATENCY: OnceLock<HistogramVec> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::new)
}

pub fn init_metrics() {
    let reg = registry();

    let success = IntCounter::new("geocode_success_total", "Successful geocode operations").unwrap();
    reg.register(Box::new(success.clone())).unwrap();
    GEOCODE_SUCCESS.set(success).unwrap();

    let failure = IntCounter::new("geocode_failure_total", "Failed geocode operations").unwrap();
    reg.register(Box::new(failure.clone())).unwrap();
    GEOCODE_FAILURE.set(failure).unwrap();

    let latency = HistogramVec::new(
        HistogramOpts::new("geocode_latency_seconds", "Geocode latency")
            .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0]),
        &["provider"],
    )
    .unwrap();
    reg.register(Box::new(latency.clone())).unwrap();
    GEOCODE_LATENCY.set(latency).unwrap();
}

pub fn record_success() {
    if let Some(c) = GEOCODE_SUCCESS.get() {
        c.inc();
    }
}

pub fn record_failure() {
    if let Some(c) = GEOCODE_FAILURE.get() {
        c.inc();
    }
}

pub fn record_latency(provider: &str, seconds: f64) {
    if let Some(h) = GEOCODE_LATENCY.get() {
        h.with_label_values(&[provider]).observe(seconds);
    }
}

// ---- HTTP endpoint ----

pub async fn serve_metrics(addr: SocketAddr) -> Result<()> {
    init_metrics();

    let make = make_service_fn(move |_| async {
        Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| async move {
            match req.uri().path() {
                "/metrics" => {
                    let encoder = TextEncoder::new();
                    let mf = registry().gather();
                    let mut buf = Vec::new();
                    encoder.encode(&mf, &mut buf).unwrap();
                    Ok::<_, hyper::Error>(
                        Response::builder()
                            .header("Content-Type", encoder.format_type())
                            .body(Body::from(buf))
                            .unwrap(),
                    )
                }
                "/health" | "/healthz" => Ok(Response::new(Body::from("ok"))),
                _ => Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap()),
            }
        }))
    });

    let server = Server::bind(&addr).serve(make);
    tracing::info!(%addr, "Prometheus metrics server listening");
    server.await?;
    Ok(())
}
