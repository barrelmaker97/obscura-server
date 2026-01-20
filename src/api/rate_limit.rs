use axum::extract::ConnectInfo;
use axum::http::Request;
use ipnetwork::IpNetwork;
use std::net::{IpAddr, SocketAddr};
use tower_governor::GovernorError;
use tower_governor::key_extractor::KeyExtractor;

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
