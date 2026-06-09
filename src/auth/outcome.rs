use std::any::Any;

use crate::Extensions;

/// The outcome of an auth handler: accept or reject, plus any typed data to
/// attach to the session on accept.
///
/// Plain `-> bool` closures keep working through `From<bool>`; reach for
/// [`accept`](Self::accept) and [`with`](Self::with) only when you want to
/// stash data for the handler to read later.
///
/// ```
/// # use shenron::Auth;
/// struct Account { id: u32 }
/// let _ = Auth::accept().with(Account { id: 7 });
/// ```
pub struct Auth {
    accepted: bool,
    extensions: Extensions,
}

impl Auth {
    #[must_use]
    pub fn accept() -> Self {
        Self {
            accepted: true,
            extensions: Extensions::default(),
        }
    }

    #[must_use]
    pub fn reject() -> Self {
        Self {
            accepted: false,
            extensions: Extensions::default(),
        }
    }

    /// Attach a typed value, readable in the handler via
    /// [`Session::get`](crate::Session::get). No effect on a rejected outcome —
    /// rejected data is dropped.
    #[must_use]
    pub fn with<T: Any + Send + Sync>(mut self, value: T) -> Self {
        self.extensions.insert(value);
        self
    }

    pub(crate) const fn accepted(&self) -> bool {
        self.accepted
    }

    pub(crate) fn into_extensions(self) -> Extensions {
        self.extensions
    }
}

impl From<bool> for Auth {
    fn from(accepted: bool) -> Self {
        if accepted { Self::accept() } else { Self::reject() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Account(u32);

    #[test]
    fn accept_and_reject() {
        assert!(Auth::accept().accepted());
        assert!(!Auth::reject().accepted());
    }

    #[test]
    fn from_bool() {
        assert!(Auth::from(true).accepted());
        assert!(!Auth::from(false).accepted());
    }

    #[test]
    fn with_attaches_data() {
        let ext = Auth::accept().with(Account(42)).into_extensions();

        assert_eq!(ext.get::<Account>().map(|a| a.0), Some(42));
    }
}
