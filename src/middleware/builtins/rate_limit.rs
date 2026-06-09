use std::{num::NonZeroU32, sync::Arc};

use governor::{
    Quota, RateLimiter as GovernorLimiter, clock::DefaultClock, middleware::NoOpMiddleware,
    state::keyed::DashMapStateStore,
};

use crate::{Middleware, Next, Result, Session};

type KeyedLimiter =
    GovernorLimiter<String, DashMapStateStore<String>, DefaultClock, NoOpMiddleware>;

/// Per-IP rate limiting for established sessions.
///
/// Note: this runs as middleware, so it only sees sessions that have already
/// authenticated and opened a channel. It throttles abusive *session* rates,
/// not raw connection or failed-auth floods — pair it with network-level
/// limits (e.g. a firewall) if you need to defend the handshake itself.
#[derive(Clone)]
pub struct RateLimiter {
    quota: Quota,
    limiter: Arc<KeyedLimiter>,
}

impl RateLimiter {
    /// Allow `count` new sessions per second per IP
    ///
    /// # Panics
    ///
    /// Panics if `count` is zero
    #[must_use]
    pub fn per_second(count: u32) -> Self {
        Self::from_quota(Quota::per_second(non_zero(count)))
    }

    /// Allow `count` new sessions per minute per IP
    ///
    /// # Panics
    ///
    /// Panics if `count` is zero
    #[must_use]
    pub fn per_minute(count: u32) -> Self {
        Self::from_quota(Quota::per_minute(non_zero(count)))
    }

    /// Allow `count` new sessions per hour per IP
    ///
    /// # Panics
    ///
    /// Panics if `count` is zero
    #[must_use]
    pub fn per_hour(count: u32) -> Self {
        Self::from_quota(Quota::per_hour(non_zero(count)))
    }

    /// Cap how many sessions an IP can start at once, independent of the
    /// sustained rate. Defaults to the sustained `count` when not set.
    ///
    /// # Panics
    ///
    /// Panics if `count` is zero
    #[must_use]
    pub fn burst(self, count: u32) -> Self {
        Self::from_quota(self.quota.allow_burst(non_zero(count)))
    }

    fn from_quota(quota: Quota) -> Self {
        Self {
            quota,
            limiter: Arc::new(GovernorLimiter::dashmap(quota)),
        }
    }
}

const fn non_zero(count: u32) -> NonZeroU32 {
    NonZeroU32::new(count).expect("count cannot be 0")
}

impl Middleware for RateLimiter {
    async fn handle(&self, session: &'_ mut Session, next: Next<'_>) -> Result {
        let key = session.remote_addr().ip().to_string();

        if self.limiter.check_key(&key).is_err() {
            session
                .write_stderr_str("Rate limit exceeded, try again later\n")
                .await?;

            return session.exit(1);
        }

        next.run(session).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(limiter: &RateLimiter, key: &str) -> bool {
        limiter.limiter.check_key(&key.to_string()).is_ok()
    }

    #[test]
    fn burst_defaults_to_sustained_count() {
        let limiter = RateLimiter::per_minute(3);

        assert!(allowed(&limiter, "10.0.0.1"));
        assert!(allowed(&limiter, "10.0.0.1"));
        assert!(allowed(&limiter, "10.0.0.1"));
        assert!(!allowed(&limiter, "10.0.0.1"));
    }

    #[test]
    fn burst_caps_below_sustained_count() {
        let limiter = RateLimiter::per_hour(100).burst(2);

        assert!(allowed(&limiter, "10.0.0.1"));
        assert!(allowed(&limiter, "10.0.0.1"));
        assert!(!allowed(&limiter, "10.0.0.1"));
    }

    #[test]
    fn keys_are_independent() {
        let limiter = RateLimiter::per_minute(1);

        assert!(allowed(&limiter, "10.0.0.1"));
        assert!(!allowed(&limiter, "10.0.0.1"));
        assert!(allowed(&limiter, "10.0.0.2"));
    }
}
