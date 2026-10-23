// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

//! Per-client fixed-window rate limiting for state-changing (POST) requests.

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);
/// Expired windows are pruned once the table holds this many clients.
const PRUNE_THRESHOLD: usize = 10_000;

pub struct RateLimiter {
    /// Allowed writes per client per minute; 0 disables the limit.
    per_minute: u32,
    /// Use the last `X-Forwarded-For` entry as the client address. Enable only
    /// behind a reverse proxy that appends the real client address.
    trust_forwarded_for: bool,
    windows: Mutex<HashMap<IpAddr, Window>>,
}

struct Window {
    start: Instant,
    count: u32,
}

impl RateLimiter {
    pub fn new(per_minute: u32, trust_forwarded_for: bool) -> Self {
        Self {
            per_minute,
            trust_forwarded_for,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Record a write from `client`. Returns the time until the client may
    /// write again if it is over the limit.
    fn check(&self, client: IpAddr, now: Instant) -> Result<(), Duration> {
        if self.per_minute == 0 {
            return Ok(());
        }
        let mut windows = self.windows.lock().unwrap();
        if windows.len() >= PRUNE_THRESHOLD {
            windows.retain(|_, w| now.duration_since(w.start) < WINDOW);
        }
        let window = windows.entry(client).or_insert(Window {
            start: now,
            count: 0,
        });
        if now.duration_since(window.start) >= WINDOW {
            window.start = now;
            window.count = 0;
        }
        if window.count >= self.per_minute {
            return Err(WINDOW - now.duration_since(window.start));
        }
        window.count += 1;
        Ok(())
    }

    fn client_key(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> IpAddr {
        let forwarded = self
            .trust_forwarded_for
            .then(|| last_forwarded_for(headers))
            .flatten();
        // Without connection info all clients share one bucket.
        let ip = forwarded
            .or(peer)
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        match ip {
            // A single IPv6 client typically controls a whole /64.
            IpAddr::V6(v6) => {
                let masked = u128::from(v6) & !((1u128 << 64) - 1);
                IpAddr::V6(Ipv6Addr::from(masked))
            }
            v4 => v4,
        }
    }
}

fn last_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get_all("x-forwarded-for")
        .iter()
        .next_back()?
        .to_str()
        .ok()?
        .rsplit(',')
        .next()?
        .trim()
        .parse()
        .ok()
}

pub async fn limit_writes(
    State(limiter): State<Arc<RateLimiter>>,
    req: Request,
    next: Next,
) -> Response {
    if req.method() != Method::POST {
        return next.run(req).await;
    }
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());
    let client = limiter.client_key(req.headers(), peer);
    match limiter.check(client, Instant::now()) {
        Ok(()) => next.run(req).await,
        Err(retry_after) => (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                header::RETRY_AFTER,
                retry_after.as_secs().max(1).to_string(),
            )],
            "Too many requests, please try again later",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn allows_up_to_limit_then_rejects() {
        let limiter = RateLimiter::new(2, false);
        let now = Instant::now();
        assert!(limiter.check(ip("10.0.0.1"), now).is_ok());
        assert!(limiter.check(ip("10.0.0.1"), now).is_ok());
        let retry = limiter.check(ip("10.0.0.1"), now).unwrap_err();
        assert_eq!(retry, WINDOW);
        // Other clients have their own budget.
        assert!(limiter.check(ip("10.0.0.2"), now).is_ok());
    }

    #[test]
    fn window_resets_after_a_minute() {
        let limiter = RateLimiter::new(1, false);
        let now = Instant::now();
        assert!(limiter.check(ip("10.0.0.1"), now).is_ok());
        assert!(limiter.check(ip("10.0.0.1"), now).is_err());
        assert!(limiter.check(ip("10.0.0.1"), now + WINDOW).is_ok());
    }

    #[test]
    fn zero_disables_limit() {
        let limiter = RateLimiter::new(0, false);
        let now = Instant::now();
        for _ in 0..100 {
            assert!(limiter.check(ip("10.0.0.1"), now).is_ok());
        }
    }

    #[test]
    fn prunes_expired_windows() {
        let limiter = RateLimiter::new(1, false);
        let now = Instant::now();
        for i in 0..PRUNE_THRESHOLD as u32 {
            limiter.check(IpAddr::V4(Ipv4Addr::from(i)), now).unwrap();
        }
        limiter.check(ip("10.0.0.1"), now + WINDOW).unwrap();
        assert_eq!(limiter.windows.lock().unwrap().len(), 1);
    }

    #[test]
    fn ipv6_clients_are_grouped_by_64_prefix() {
        let limiter = RateLimiter::new(1, false);
        let headers = HeaderMap::new();
        let a = limiter.client_key(&headers, Some(ip("2001:db8:1:2::1")));
        let b = limiter.client_key(&headers, Some(ip("2001:db8:1:2:ffff::9")));
        let c = limiter.client_key(&headers, Some(ip("2001:db8:1:3::1")));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn forwarded_for_used_only_when_trusted() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "1.2.3.4, 5.6.7.8".parse().unwrap());
        let peer = Some(ip("127.0.0.1"));
        let untrusted = RateLimiter::new(1, false);
        assert_eq!(untrusted.client_key(&headers, peer), ip("127.0.0.1"));
        let trusted = RateLimiter::new(1, true);
        assert_eq!(trusted.client_key(&headers, peer), ip("5.6.7.8"));
    }

    #[test]
    fn falls_back_to_shared_key_without_peer() {
        let limiter = RateLimiter::new(1, true);
        let key = limiter.client_key(&HeaderMap::new(), None);
        assert_eq!(key, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }
}
