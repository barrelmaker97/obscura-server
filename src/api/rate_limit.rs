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
        let trusted_proxies = trusted_proxies_str
            .split(',')
            .filter_map(|s| s.trim().parse::<IpNetwork>().ok())
            .collect();
        Self { trusted_proxies }
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

        // If peer is not trusted, we don't even look at X-Forwarded-For
        if !self.is_trusted(&peer_ip) {
            return Ok(peer_ip);
        }

        // If peer is trusted, try X-Forwarded-For header
        let xff = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok());

        if let Some(xff_val) = xff {
            // Walk the chain from right to left
            // The rightmost IP is the one that talked to our trusted proxy.
            let ips: Vec<IpAddr> = xff_val
                .split(',')
                .filter_map(|s| s.trim().parse::<IpAddr>().ok())
                .collect();

            for ip in ips.into_iter().rev() {
                if !self.is_trusted(&ip) {
                    return Ok(ip);
                }
            }
        }

        // Fallback to peer IP if header is empty or all IPs are trusted
        Ok(peer_ip)
    }
}
