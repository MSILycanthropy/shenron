use std::sync::Arc;

use russh::keys::PrivateKey;

use crate::{App, Result};

#[derive(Default)]
pub struct Server<A> {
    addr: Option<String>,
    keys: Vec<PrivateKey>,
    app: Option<Arc<A>>,
}

impl<A> Server<A>
where
    A: App,
{
    #[must_use]
    pub fn bind(mut self, addr: impl Into<String>) -> Self {
        self.addr = Some(addr.into());
        self
    }

    #[must_use]
    pub fn host_key(mut self, key: PrivateKey) -> Self {
        self.keys.push(key);
        self
    }

    /// # Errors
    ///
    /// Will return `Err` if russh failes to load the secret key
    pub fn host_key_file(self, path: impl AsRef<std::path::Path>) -> Result<Self> {
        let key = russh::keys::load_secret_key(path, None)?;

        Ok(self.host_key(key))
    }

    #[must_use]
    pub fn app(mut self, app: A) -> Self {
        self.app = Some(Arc::new(app));

        self
    }
}
