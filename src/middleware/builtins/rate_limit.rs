use std::{num::NonZeroU32, sync::Arc};

use governor::{
    Quota, RateLimiter as GovernorLimiter, clock::DefaultClock, middleware::NoOpMiddleware,
    state::keyed::DashMapStateStore,
};

use crate::{Middleware, Next, Result, Session};

type KeyedLimiter =
    GovernorLimiter<String, DashMapStateStore<String>, DefaultClock, NoOpMiddleware>;

/// Middleware to add rate limiting per session
#[derive(Clone)]
pub struct RateLimiter {
    limiter: Arc<KeyedLimiter>,
}

impl RateLimiter {
    #[must_use]
    /// Create a rate limiter that allows `count` connections per `period` per IP
    ///
    /// # Panics
    ///
    /// Panics if
    ///   - period is zero
    ///   - count is zero
    pub fn new(count: u32, period: std::time::Duration) -> Self {
        let quota = Quota::with_period(period)
            .expect("period cannot be 0")
            .allow_burst(NonZeroU32::new(count).expect("count cannot be 0"));

        Self {
            limiter: Arc::new(GovernorLimiter::dashmap(quota)),
        }
    }

    /// Create a rate limiter that allows `count` connections per second per IP
    #[must_use]
    pub fn per_second(count: u32) -> Self {
        Self::new(count, std::time::Duration::from_secs(1))
    }

    /// Create a rate limiter that allows `count` connections per minute per IP
    #[must_use]
    pub fn per_minute(count: u32) -> Self {
        Self::new(count, std::time::Duration::from_secs(60))
    }
}

impl Middleware for RateLimiter {
    async fn handle(&self, session: Session, next: Next) -> Result<Session> {
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
