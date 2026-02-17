use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use ipnetwork::IpNetwork;
use opentelemetry::{KeyValue, global, metrics::Counter};
use std::net::{IpAddr, SocketAddr};
use tower_governor::GovernorError;
use tower_governor::key_extractor::KeyExtractor;
use tracing::warn;

#[derive(Clone, Debug)]
pub struct Metrics {
    pub(crate) decisions_total: Counter<u64>,
}

impl Metrics {
    #[must_use]
    pub(crate) fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            decisions_total: meter
                .u64_counter("obscura_rate_limit_decisions_total")
                .with_description("Rate limit decisions (allowed/throttled)")
                .build(),
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct IpKeyExtractor {
    pub(crate) trusted_proxies: Vec<IpNetwork>,
}

impl IpKeyExtractor {
    #[must_use]
    pub(crate) const fn new(trusted_proxies: Vec<IpNetwork>) -> Self {
        Self { trusted_proxies }
    }

    #[must_use]
    pub(crate) fn identify_client_ip(&self, headers: &axum::http::HeaderMap, peer_addr: IpAddr) -> IpAddr {
        if !self.is_trusted(&peer_addr) {
            return peer_addr;
        }

        let xff = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());

        if let Some(xff_val) = xff
            && let Some(real_ip) =
                xff_val.rsplit(',').filter_map(|s| s.trim().parse::<IpAddr>().ok()).find(|ip| !self.is_trusted(ip))
        {
            return real_ip;
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

#[derive(Clone, Debug)]
pub struct RateLimitService {
    pub extractor: IpKeyExtractor,
    pub metrics: Metrics,
}

impl RateLimitService {
    #[must_use]
    pub fn new(trusted_proxies: Vec<IpNetwork>) -> Self {
        Self { extractor: IpKeyExtractor::new(trusted_proxies), metrics: Metrics::new() }
    }

    pub fn log_decision(&self, status: StatusCode, ratelimit_after: Option<String>) {
        let label = if status == StatusCode::TOO_MANY_REQUESTS {
            if let Some(after) = ratelimit_after {
                warn!("Rate limit exceeded (retry allowed after {}s)", after);
            }
            "throttled"
        } else {
            "allowed"
        };

        self.metrics.decisions_total.add(1, &[KeyValue::new("status", label)]);
    }
}
