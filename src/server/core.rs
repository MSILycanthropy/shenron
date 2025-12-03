use std::{pin::Pin, sync::Arc, time::Duration};

use russh::{
    keys::{PrivateKey, PublicKey},
    server::{Config, Server as _},
};

use crate::{
    Handler, Middleware,
    auth::AuthConfig,
    middleware::{self, ErasedHandler, ErasedMiddleware},
    server::ShenronServer,
};

type ShutdownFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

#[derive(Default)]
pub struct Server {
    addr: Option<String>,
    keys: Vec<PrivateKey>,
    middleware: Vec<Arc<dyn ErasedMiddleware>>,
    auth: AuthConfig,
    app: Option<Arc<dyn ErasedHandler>>,
    shutdown: Option<ShutdownFuture>,
    auth_timeout: Option<Duration>,
    inactivity_timeout: Option<Duration>,
    banner: Option<String>,
    keepalive_interval: Option<Duration>,
    keepalive_max: Option<usize>,
}

impl Server {
    /// Create a new instance of a Server
    ///
    /// # Panics
    ///
    /// Panics if creating the `host_key` fails
    #[must_use]
    pub fn new() -> Self {
        let instance = Self::default();

        let key = russh::keys::PrivateKey::random(
            &mut rand::rngs::OsRng,
            russh::keys::Algorithm::Ed25519,
        )
        .expect("Failed to create key");

        instance.host_key(key)
    }

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

    /// Add a host key from file
    ///
    /// # Errors
    ///
    /// Returns `Err` if the key file cannot be loaded
    pub fn host_key_file(self, path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        let key = russh::keys::load_secret_key(path, None)?;

        Ok(self.host_key(key))
    }

    #[must_use]
    pub fn banner(mut self, banner: impl Into<String>) -> Self {
        self.banner = Some(banner.into());
        self
    }

    /// Add a banner from a file
    ///
    /// # Errors
    ///
    /// Reeturns `Err` if the banner file cannot be loaded
    pub fn banner_file(self, path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        let banner = std::fs::read_to_string(path)?;

        Ok(self.banner(banner))
    }

    #[must_use]
    pub const fn keepalive_interval(mut self, duration: Duration) -> Self {
        self.keepalive_interval = Some(duration);

        self
    }

    #[must_use]
    pub const fn keepalive_max(mut self, retries: usize) -> Self {
        self.keepalive_max = Some(retries);

        self
    }

    /// Add a middlware to the middlware stack
    ///
    /// Middlware are executed outside-in: the first middleware
    /// is the outermost (ie it sees the session first and the result last)
    #[must_use]
    pub fn with<M: Middleware + Clone>(mut self, middleware: M) -> Self {
        self.middleware.push(Arc::new(middleware));

        self
    }

    /// Set a password authentication handler
    ///
    /// The handler receives the username and password and returns
    /// a boolean representing if the connection is accepted or rejected
    ///
    /// # Example
    ///
    /// ```rust
    /// Server::new()
    ///     .password_auth(|user, password| async move {
    ///         user == "admin" && password == "admin"
    ///     })
    /// ```
    #[must_use]
    pub fn password_auth<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(String, String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.auth.password = Some(Arc::new(handler));

        self
    }

    /// Set a public key authentication handler
    ///
    /// The handler receives the username and public key, and returns
    /// a boolean representing if the connection is accepted or rejected.
    ///
    /// # Example
    /// ```rust
    ///  Server::new()
    ///     .pubkey_auth(|user, key| async move {
    ///         allowed_keys.contains(&key.fingerprint())
    ///     })
    /// ```
    #[must_use]
    pub fn pubkey_auth<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(String, PublicKey) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.auth.pubkey = Some(Arc::new(handler));

        self
    }

    #[must_use]
    pub const fn auth_timeout(mut self, duration: Duration) -> Self {
        self.auth_timeout = Some(duration);

        self
    }

    #[must_use]
    pub const fn inactivity_timeout(mut self, duration: Duration) -> Self {
        self.inactivity_timeout = Some(duration);

        self
    }

    /// Set the application handler
    #[must_use]
    pub fn app<H: Handler>(mut self, handler: H) -> Self {
        let chain = middleware::build_chain(handler, std::mem::take(&mut self.middleware));

        self.app = Some(chain);

        self
    }

    /// Set a graceful shutdown signal
    ///
    /// When the future completes, the server will stop accepting new connections.
    ///
    /// # Example
    ///
    /// ```rust
    /// Server::new()
    ///     .bind("127.0.0.1:2222")
    ///     .host_key_file("host_key")?
    ///     .with_graceful_shutdown(async {
    ///         tokio::signal::ctrl_c().await.ok();
    ///     })
    ///     .app(app)
    ///     .serve()
    ///     .await
    /// ```
    #[must_use]
    pub fn shutdown_signal<F>(mut self, signal: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.shutdown = Some(Box::pin(signal));
        self
    }

    /// Start the server and listen for connections
    ///
    /// # Errors
    ///
    /// Returns `Err` if
    /// - No bind address was specified
    /// - No host keys were supplied
    /// - The server failed to start
    pub async fn serve(self) -> crate::Result<()> {
        let config = self.config();

        let addr = self
            .addr
            .ok_or_else(|| crate::Error::Config("No bind address specified".into()))?;

        if self.keys.is_empty() {
            return Err(crate::Error::Config("No host keys specified".into()));
        }

        let handler = self
            .app
            .ok_or_else(|| crate::Error::Config("No app handler specified".into()))?;

        let auth = Arc::new(self.auth);
        let mut sh = ShenronServer {
            handler,
            auth,
            banner: self.banner,
        };

        match self.shutdown {
            Some(shutdown) => {
                tokio::select! {
                    result = sh.run_on_address(config, addr) => {
                        result?;
                    }
                    () = shutdown => {
                        tracing::info!("Shutdown signal received");
                    }
                }
            }
            None => {
                sh.run_on_address(config, addr).await?;
            }
        }

        Ok(())
    }

    fn config(&self) -> Arc<Config> {
        let mut config = Config::default();

        config.keys.clone_from(&self.keys);

        if !self.auth.is_empty() {
            config.methods = self.auth.methods();
        }

        if let Some(timeout) = self.auth_timeout {
            config.auth_rejection_time = timeout;
            config.auth_rejection_time_initial = Some(timeout);
        }

        if let Some(timeout) = self.inactivity_timeout {
            config.inactivity_timeout = Some(timeout);
        }

        config.keepalive_interval = self.keepalive_interval;

        if let Some(max) = self.keepalive_max {
            config.keepalive_max = max;
        }

        Arc::new(config)
    }
}
