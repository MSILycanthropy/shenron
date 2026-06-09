use std::{path::Path, pin::Pin, sync::Arc, time::Duration};

use russh::{
    keys::{PrivateKey, PublicKey},
    server::{Config, Server as _},
};

use crate::{
    Middleware, Session,
    auth::AuthConfig,
    middleware::{self, ErasedMiddleware},
    server::{ShenronServer, keygen, keygen::HostKeyOptions},
};

type ShutdownFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Where the default host key is generated when none is configured.
/// Matches Wish, which writes `id_ed25519` to the working directory.
const DEFAULT_HOST_KEY_PATH: &str = "id_ed25519";

#[derive(Default)]
pub struct Server {
    addr: Option<String>,
    keys: Vec<PrivateKey>,
    middleware: Vec<Arc<dyn ErasedMiddleware>>,
    auth: AuthConfig,
    shutdown: Option<ShutdownFuture>,
    auth_rejection_delay: Option<Duration>,
    auth_rejection_delay_initial: Option<Duration>,
    inactivity_timeout: Option<Duration>,
    banner: Option<String>,
    keepalive_interval: Option<Duration>,
    keepalive_max: Option<usize>,
}

impl Server {
    /// Create a new instance of a Server
    ///
    /// No host key is generated here. If none is configured before
    /// [`serve`](Self::serve), a default Ed25519 key is generated and persisted
    /// to [`DEFAULT_HOST_KEY_PATH`] (and reused on the next start).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
    pub fn host_key_file(self, path: impl AsRef<Path>) -> crate::Result<Self> {
        let key = russh::keys::load_secret_key(path, None)?;

        Ok(self.host_key(key))
    }

    /// Add a host key from a path, generating and persisting one if it is missing
    ///
    /// On first run this writes a new Ed25519 private key to `path` and its
    /// public key to `<path>.pub`; later runs load the existing key so the
    /// server keeps a stable identity across restarts.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the key cannot be loaded, generated, or written
    pub fn host_key_path(self, path: impl AsRef<Path>) -> crate::Result<Self> {
        let path = path.as_ref();

        let key = keygen::load_or_generate(path, HostKeyOptions::default())?;

        Ok(self.host_key(key))
    }

    /// Add a host key from a path, controlling the generated key's algorithm
    /// and (optionally) encrypting it with a passphrase
    ///
    /// Behaves like [`host_key_path`](Self::host_key_path): an existing key is
    /// loaded (using the passphrase to decrypt if set), otherwise a new key of
    /// the chosen algorithm is generated and persisted. Each added key makes
    /// its algorithm available for negotiation; the client picks per the SSH
    /// host-key preference order.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the key cannot be loaded, generated, or written
    pub fn host_key_path_with(
        self,
        path: impl AsRef<Path>,
        options: HostKeyOptions,
    ) -> crate::Result<Self> {
        let key = keygen::load_or_generate(path.as_ref(), options)?;

        Ok(self.host_key(key))
    }

    /// Add a passphrase-encrypted host key from a file
    ///
    /// # Errors
    ///
    /// Returns `Err` if the key cannot be loaded or decrypted
    pub fn host_key_file_with(
        self,
        path: impl AsRef<Path>,
        passphrase: impl AsRef<str>,
    ) -> crate::Result<Self> {
        let key = russh::keys::load_secret_key(path, Some(passphrase.as_ref()))?;

        Ok(self.host_key(key))
    }

    /// Add a host key from raw OpenSSH/PEM bytes (Wish `WithHostKeyPEM`)
    ///
    /// # Errors
    ///
    /// Returns `Err` if the bytes cannot be decoded into a private key
    pub fn host_key_pem(self, pem: impl AsRef<[u8]>) -> crate::Result<Self> {
        let key = keygen::from_pem(pem.as_ref(), None)?;

        Ok(self.host_key(key))
    }

    /// Add a passphrase-encrypted host key from raw OpenSSH/PEM bytes
    ///
    /// # Errors
    ///
    /// Returns `Err` if the bytes cannot be decoded or decrypted
    pub fn host_key_pem_with(
        self,
        pem: impl AsRef<[u8]>,
        passphrase: impl AsRef<str>,
    ) -> crate::Result<Self> {
        let key = keygen::from_pem(pem.as_ref(), Some(passphrase.as_ref()))?;

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
    /// Returns `Err` if the banner file cannot be loaded
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

    /// Add a middleware to the middleware stack
    ///
    /// Middleware are executed outside-in: the first middleware
    /// is the outermost (ie it sees the session first and the result last)
    #[must_use]
    pub fn with<M: Middleware>(mut self, middleware: M) -> Self {
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
    /// ```no_run
    /// # use shenron::Server;
    /// let _server = Server::new()
    ///     .password_auth(|user, password| async move {
    ///         user == "admin" && password == "admin"
    ///     });
    /// ```
    #[must_use]
    pub fn password_auth<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(String, String) -> Fut + Send + Sync + 'static,
        Fut: Future + Send + 'static,
        Fut::Output: Into<crate::Auth>,
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
    /// ```no_run
    /// # use shenron::Server;
    /// # use russh::keys::HashAlg;
    /// let _server = Server::new()
    ///     .pubkey_auth(|user, key| async move {
    ///         key.fingerprint(HashAlg::Sha256).to_string() == "SHA256:abc123..."
    ///     });
    /// ```
    #[must_use]
    pub fn pubkey_auth<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(String, PublicKey) -> Fut + Send + Sync + 'static,
        Fut: Future + Send + 'static,
        Fut::Output: Into<crate::Auth>,
    {
        self.auth.pubkey = Some(Arc::new(handler));

        self
    }

    /// Constant delay before every *failed* auth attempt is answered.
    ///
    /// This is a brute-force throttle and timing-side-channel mitigation, not
    /// a deadline: rejections always take exactly this long regardless of why
    /// they failed, so an auth handler's timing can't leak which usernames
    /// exist. russh defaults to 1 second — don't lower it without a reason.
    #[must_use]
    pub const fn auth_rejection_delay(mut self, duration: Duration) -> Self {
        self.auth_rejection_delay = Some(duration);

        self
    }

    /// Delay for the *first* rejected auth attempt on a connection.
    ///
    /// OpenSSH clients routinely probe with a `none` auth request to discover
    /// the allowed methods; that probe is rejected by design. A shorter first
    /// delay lets the probe fail fast so legitimate logins aren't slowed,
    /// while later failures still wait the full
    /// [`auth_rejection_delay`](Self::auth_rejection_delay).
    #[must_use]
    pub const fn auth_rejection_delay_initial(mut self, duration: Duration) -> Self {
        self.auth_rejection_delay_initial = Some(duration);

        self
    }

    #[must_use]
    pub const fn inactivity_timeout(mut self, duration: Duration) -> Self {
        self.inactivity_timeout = Some(duration);

        self
    }

    /// Add a terminal application as the innermost layer.
    ///
    /// Sugar for [`with(terminal(app))`](Self::with): the app is just a
    /// middleware that ignores the rest of the chain.
    ///
    /// Add it last. Middleware registered *before* it still run their
    /// after-`next` work as the chain unwinds (e.g. `elapsed`, `Comment`);
    /// middleware registered *after* it nest inside the app and never run,
    /// since the app ignores `next`.
    #[must_use]
    pub fn app<F>(self, app: F) -> Self
    where
        F: AsyncFn(&mut Session) -> crate::Result + Send + Sync + 'static,
        for<'a> <F as std::ops::AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
    {
        self.with(middleware::terminal(app))
    }

    /// Set a graceful shutdown signal
    ///
    /// When the future completes, the server will stop accepting new connections.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use shenron::{Server, Session};
    /// # async fn app(session: &mut Session) -> shenron::Result {
    /// #     session.exit(0)
    /// # }
    /// # async fn run() -> shenron::Result<()> {
    /// Server::new()
    ///     .bind("127.0.0.1:2222")
    ///     .shutdown_signal(async {
    ///         tokio::signal::ctrl_c().await.ok();
    ///     })
    ///     .app(app)
    ///     .serve()
    ///     .await
    /// # }
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
    /// - A default host key had to be generated and writing it failed
    /// - The server failed to start
    pub async fn serve(mut self) -> crate::Result<()> {
        if self.keys.is_empty() {
            self = self.host_key_path(DEFAULT_HOST_KEY_PATH)?;
        }

        let config = self.config();

        let addr = self
            .addr
            .ok_or_else(|| crate::Error::Config("No bind address specified".into()))?;

        let handler = middleware::build_chain(std::mem::take(&mut self.middleware));

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

        if let Some(delay) = self.auth_rejection_delay {
            config.auth_rejection_time = delay;
        }

        if let Some(delay) = self.auth_rejection_delay_initial {
            config.auth_rejection_time_initial = Some(delay);
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
