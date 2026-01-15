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
    pub fn new(trusted_proxies_str: &str) -> Self {
        let trusted_proxies =
            trusted_proxies_str.split(',').filter_map(|s| s.trim().parse::<IpNetwork>().ok()).collect();
        Self { trusted_proxies }
    }

    fn is_trusted(&self, ip: &IpAddr) -> bool {
        self.trusted_proxies.iter().any(|net| net.contains(*ip))
    }
}

impl KeyExtractor for IpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        // Start with the immediate peer (e.g., the Ingress or Load Balancer)
        let peer_ip = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            .ok_or(GovernorError::UnableToExtractKey)?;

        // Only trust X-Forwarded-For if the request comes from a known proxy.
        // If the peer is untrusted, we assume they are the client.
        if !self.is_trusted(&peer_ip) {
            return Ok(peer_ip);
        }

        let xff = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok());

        if let Some(xff_val) = xff {
            // Walk the chain from right to left (most recent to original).
            // We skip any IPs that belong to our own infrastructure (trusted proxies).
            // The first IP we encounter that IS NOT trusted is considered the real client.
            let ips: Vec<IpAddr> = xff_val.split(',').filter_map(|s| s.trim().parse::<IpAddr>().ok()).collect();

            for ip in ips.into_iter().rev() {
                if !self.is_trusted(&ip) {
                    return Ok(ip);
                }
            }
        }

        // Fallback to peer IP if header is missing or only contains trusted proxies
        Ok(peer_ip)
    }
}
