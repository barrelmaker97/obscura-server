use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use ipnetwork::IpNetwork;
use opentelemetry::{global, KeyValue};
use std::net::{IpAddr, SocketAddr};
use tower_governor::GovernorError;
use tower_governor::key_extractor::KeyExtractor;
use tracing::warn;

#[derive(Clone)]
pub struct IpKeyExtractor {
    trusted_proxies: Vec<IpNetwork>,
}

impl IpKeyExtractor {
    pub fn new(trusted_proxies: Vec<IpNetwork>) -> Self {
        Self { trusted_proxies }
    }

    pub fn identify_client_ip(&self, headers: &axum::http::HeaderMap, peer_addr: IpAddr) -> IpAddr {
        // Only trust X-Forwarded-For if the request comes from a known proxy.
        if !self.is_trusted(&peer_addr) {
            return peer_addr;
        }

        let xff = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());

        if let Some(xff_val) = xff {
            // Walk the chain from right to left (most recent to original).
            // We skip any IPs that belong to our own infrastructure (trusted proxies).
            // The first IP we encounter that IS NOT trusted is considered the real client.
            if let Some(real_ip) =
                xff_val.rsplit(',').filter_map(|s| s.trim().parse::<IpAddr>().ok()).find(|ip| !self.is_trusted(ip))
            {
                return real_ip;
            }
        }

        peer_addr
    }

    fn is_trusted(&self, ip: &IpAddr) -> bool {
        self.trusted_proxies.iter().any(|net| net.contains(*ip))
    }
}

impl KeyExtractor for IpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        let peer_ip = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            .ok_or(GovernorError::UnableToExtractKey)?;

        Ok(self.identify_client_ip(req.headers(), peer_ip))
    }
}

pub async fn log_rate_limit_events(req: Request<Body>, next: Next) -> Response {
    // We must extract information BEFORE calling next.run(req), as that consumes the request.
    let mut response = next.run(req).await;

    let meter = global::meter("obscura-server");
    let counter = meter.u64_counter("rate_limit_decisions_total").with_description("Rate limit decisions (allowed/throttled)").build();

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        counter.add(1, &[KeyValue::new("status", "throttled")]);

        // Map the internal x-ratelimit-after to the standard Retry-After header
        // for better compatibility with standard HTTP clients.
        let retry_after = if let Some(after) = response.headers().get("x-ratelimit-after") {
            let after = after.clone();
            response.headers_mut().insert("retry-after", after.clone());
            after.to_str().unwrap_or("?").to_string()
        } else {
            "unknown".to_string()
        };

        warn!("Rate limit exceeded (retry allowed after {}s)", retry_after);
    } else {
        counter.add(1, &[KeyValue::new("status", "allowed")]);
    }

    response
}
