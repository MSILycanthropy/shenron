use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use russh::{
    Channel,
    keys::{PrivateKey, PublicKey},
    server::{Config, Msg, Server as _, Session as RusshSession},
};

use crate::{
    Handler,
    middleware::{self, ErasedHandler, ErasedMiddleware, Middleware},
    session::PtySize,
};

#[derive(Default)]
pub struct Server {
    addr: Option<String>,
    keys: Vec<PrivateKey>,
    middleware: Vec<Arc<dyn ErasedMiddleware>>,
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

    /// Set the handler and prepare to run the server
    pub fn app<H: Handler>(self, handler: H) -> RunnableServer {
        let chain = middleware::build_chain(handler, self.middleware);

        RunnableServer {
            addr: self.addr,
            keys: self.keys,
            handler: chain,
        }
    }
}

pub struct RunnableServer {
    addr: Option<String>,
    keys: Vec<PrivateKey>,
    handler: Arc<dyn ErasedHandler>,
}

impl RunnableServer {
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

        let config = Config {
            keys: self.keys,
            ..Default::default()
        };

        let config = Arc::new(config);

        let mut sh = ShenronServer {
            handler: self.handler,
        };

        sh.run_on_address(config, addr).await?;

        Ok(())
    }
}

struct ShenronServer {
    handler: Arc<dyn ErasedHandler>,
}

impl russh::server::Server for ShenronServer {
    type Handler = ShenronHandler;

    fn new_client(&mut self, addr: Option<SocketAddr>) -> Self::Handler {
        ShenronHandler {
            handler: Arc::clone(&self.handler),
            remote_addr: addr.unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0))),
            channel: None,
            user: None,
        }
    }
}

struct ShenronHandler {
    handler: Arc<dyn ErasedHandler>,
    remote_addr: SocketAddr,
    channel: Option<Channel<Msg>>,
    user: Option<String>,
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

    async fn auth_publickey(
        &mut self,
        user: &str,
        _public_key: &PublicKey,
    ) -> crate::Result<russh::server::Auth> {
        // TODO: configing auth

        self.user = Some(user.to_string());

        Ok(russh::server::Auth::Accept)
    }

    async fn auth_password(
        &mut self,
        user: &str,
        _password: &str,
    ) -> crate::Result<russh::server::Auth> {
        // TODO: configing auth

        self.user = Some(user.to_string());

        Ok(russh::server::Auth::Accept)
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
        let channel = self
            .channel
            .take()
            .ok_or_else(|| crate::Error::Protocol("No channel available".into()))?;

        let pty_size = PtySize {
            width: col_width,
            height: row_height,
            pixel_width: pix_width,
            pixel_height: pix_height,
        };

        let user = self.user.clone().unwrap_or_else(|| "unknown".into());

        let app_session = crate::Session::new(
            channel,
            pty_size,
            user,
            term.to_string(),
            HashMap::new(),
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
