use std::{
    net::IpAddr,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use governor::{
    Quota, RateLimiter as GovernorLimiter, clock::DefaultClock, middleware::NoOpMiddleware,
    state::keyed::DashMapStateStore,
};

use crate::{Middleware, Next, Result, Session};

type KeyedLimiter =
    GovernorLimiter<IpAddr, DashMapStateStore<IpAddr>, DefaultClock, NoOpMiddleware>;

/// Sweep expired per-IP state every this many checks. Amortized inline
/// instead of a background task: no runtime needed at construction, no task
/// lifecycle, and sweeps only happen while there is actual load.
const SWEEP_INTERVAL: u64 = 256;

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
    checks: Arc<AtomicU64>,
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
            checks: Arc::new(AtomicU64::new(0)),
        }
    }

    fn check(&self, ip: IpAddr) -> bool {
        // Without periodic eviction the per-IP map grows forever (one entry
        // per address ever seen); retain_recent drops entries whose quota has
        // fully replenished.
        if self
            .checks
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(SWEEP_INTERVAL)
        {
            self.limiter.retain_recent();
        }

        self.limiter.check_key(&ip).is_ok()
    }
}

const fn non_zero(count: u32) -> NonZeroU32 {
    NonZeroU32::new(count).expect("count cannot be 0")
}

impl Middleware for RateLimiter {
    async fn handle(&self, session: &'_ mut Session, next: Next<'_>) -> Result {
        if !self.check(session.remote_addr().ip()) {
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

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn burst_defaults_to_sustained_count() {
        let limiter = RateLimiter::per_minute(3);

        assert!(limiter.check(ip(1)));
        assert!(limiter.check(ip(1)));
        assert!(limiter.check(ip(1)));
        assert!(!limiter.check(ip(1)));
    }

    #[test]
    fn burst_caps_below_sustained_count() {
        let limiter = RateLimiter::per_hour(100).burst(2);

        assert!(limiter.check(ip(1)));
        assert!(limiter.check(ip(1)));
        assert!(!limiter.check(ip(1)));
    }

    #[test]
    fn keys_are_independent() {
        let limiter = RateLimiter::per_minute(1);

        assert!(limiter.check(ip(1)));
        assert!(!limiter.check(ip(1)));
        assert!(limiter.check(ip(2)));
    }

    #[test]
    fn stale_entries_are_evicted() {
        let limiter = RateLimiter::per_second(1);

        assert!(limiter.check(ip(1)));
        assert_eq!(limiter.limiter.len(), 1);

        // An entry is droppable once its theoretical arrival time falls a full
        // quota-period behind now: replenish (1s) + retention window (1s).
        std::thread::sleep(std::time::Duration::from_millis(2100));
        limiter.limiter.retain_recent();

        assert_eq!(limiter.limiter.len(), 0);
    }
}
