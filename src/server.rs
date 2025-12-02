use std::{collections::HashMap, net::SocketAddr, pin::Pin, sync::Arc};

use russh::{
    Channel, MethodKind,
    keys::{PrivateKey, PublicKey},
    server::{Auth, Config, Msg, Server as _, Session as RusshSession},
};

use crate::{
    Handler,
    auth::AuthConfig,
    middleware::{self, ErasedHandler, ErasedMiddleware, Middleware},
    session::PtySize,
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
}

impl Server {
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
    pub fn host_key_file(self, path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        let key = russh::keys::load_secret_key(path, None)?;

        Ok(self.host_key(key))
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
        let addr = self
            .addr
            .ok_or_else(|| crate::Error::Config("No bind address specified".into()))?;

        if self.keys.is_empty() {
            return Err(crate::Error::Config("No host keys specified".into()));
        }

        let handler = self
            .app
            .ok_or_else(|| crate::Error::Config("No app handler specified".into()))?;

        let config = Config {
            keys: self.keys,
            ..Default::default()
        };
        let config = Arc::new(config);

        let auth = Arc::new(self.auth);

        let mut sh = ShenronServer { handler, auth };

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
}

struct ShenronServer {
    handler: Arc<dyn ErasedHandler>,
    auth: Arc<AuthConfig>,
}

impl russh::server::Server for ShenronServer {
    type Handler = ShenronHandler;

    fn new_client(&mut self, addr: Option<SocketAddr>) -> Self::Handler {
        ShenronHandler {
            handler: Arc::clone(&self.handler),
            remote_addr: addr.unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0))),
            channel: None,
            user: None,
            auth: Arc::clone(&self.auth),
            env: HashMap::new(),
            pty: None,
        }
    }
}

struct ShenronHandler {
    handler: Arc<dyn ErasedHandler>,
    remote_addr: SocketAddr,
    channel: Option<Channel<Msg>>,
    user: Option<String>,
    auth: Arc<AuthConfig>,
    env: HashMap<String, String>,
    pty: Option<(String, PtySize)>,
}

impl russh::server::Handler for ShenronHandler {
    type Error = crate::Error;

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut RusshSession,
    ) -> crate::Result<bool> {
        self.channel = Some(channel);

        Ok(true)
    }

    async fn auth_publickey(&mut self, user: &str, public_key: &PublicKey) -> crate::Result<Auth> {
        let mut accept = || -> crate::Result<Auth> {
            self.user = Some(user.to_string());

            Ok(Auth::Accept)
        };

        let rejection = Ok(Auth::Reject {
            proceed_with_methods: Some([MethodKind::Password].as_slice().into()),
            partial_success: false,
        });

        if let Some(ref handler) = self.auth.pubkey {
            if handler.verify(user, public_key).await {
                return accept();
            }

            return rejection;
        }

        if self.auth.is_empty() {
            return accept();
        }

        rejection
    }

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> crate::Result<russh::server::Auth> {
        let mut accept = || -> crate::Result<Auth> {
            self.user = Some(user.to_string());

            Ok(Auth::Accept)
        };

        let rejection = Ok(Auth::Reject {
            proceed_with_methods: None,
            partial_success: false,
        });

        if let Some(ref handler) = self.auth.password {
            if handler.verify(user, password).await {
                return accept();
            }

            return rejection;
        }

        if self.auth.is_empty() {
            return accept();
        }

        rejection
    }

    async fn env_request(
        &mut self,
        _channel: russh::ChannelId,
        variable_name: &str,
        variable_value: &str,
        _session: &mut RusshSession,
    ) -> crate::Result<()> {
        self.env
            .insert(variable_name.to_string(), variable_value.to_string());

        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel_id: russh::ChannelId,
        data: &[u8],
        session: &mut RusshSession,
    ) -> crate::Result<()> {
        let channel = self
            .channel
            .take()
            .ok_or_else(|| crate::Error::Protocol("No channel available".into()))?;

        let command = String::from_utf8_lossy(data).to_string();

        let kind = match self.pty.take() {
            Some((term, size)) => crate::SessionKind::Pty { term, size },
            None => crate::SessionKind::Exec { command },
        };

        let user = self.user.clone().unwrap_or_else(|| "unknown".into());

        let app_session = crate::Session::new(
            channel,
            kind,
            user,
            std::mem::take(&mut self.env),
            self.remote_addr,
        );

        let handler = Arc::clone(&self.handler);

        tokio::spawn(async move {
            if let Err(e) = handler.call(app_session).await {
                tracing::error!("Handler error: {}", e);
            }
        });

        session.channel_success(channel_id)?;

        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel_id: russh::ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut RusshSession,
    ) -> crate::Result<()> {
        self.pty = Some((
            term.to_string(),
            PtySize {
                width: col_width,
                height: row_height,
                pixel_width: pix_width,
                pixel_height: pix_height,
            },
        ));

        session.channel_success(channel_id)?;

        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel_id: russh::ChannelId,
        session: &mut RusshSession,
    ) -> crate::Result<()> {
        let channel = self
            .channel
            .take()
            .ok_or_else(|| crate::Error::Protocol("No channel available".into()))?;

        let user = self.user.clone().unwrap_or_else(|| "unknown".into());

        let kind = match self.pty.take() {
            Some((term, size)) => crate::SessionKind::Pty { term, size },
            None => crate::SessionKind::Shell,
        };

        let app_session = crate::Session::new(
            channel,
            kind,
            user,
            std::mem::take(&mut self.env),
            self.remote_addr,
        );

        let handler = Arc::clone(&self.handler);

        tokio::spawn(async move {
            if let Err(e) = handler.call(app_session).await {
                tracing::error!("Handler error: {}", e);
            }
        });

        session.channel_success(channel_id)?;

        Ok(())
    }
}
