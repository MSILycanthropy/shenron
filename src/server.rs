use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use russh::{
    Channel, ChannelId,
    keys::PrivateKey,
    server::{self, Auth, Config, Msg, Server as _, Session as RusshSession},
};
use tokio::sync::{Mutex, RwLock};

use crate::{
    App, Result,
    session::{PtySize, Session},
};

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

    /// # Errors
    ///
    /// Will return `Err` if `self.addr` is not set
    /// Will return `Err` if `self.app` is not set
    pub async fn serve(self) -> Result<()> {
        let addr = self
            .addr
            .ok_or_else(|| crate::Error::Custom("No bind address specified".into()))?;

        let app = self
            .app
            .ok_or_else(|| crate::Error::Custom("No app specified".into()))?;

        if self.keys.is_empty() {
            return Err(crate::Error::Custom("No host keys specified".into()));
        }

        let config = Config {
            keys: self.keys,
            ..Default::default()
        };

        let config = Arc::new(config);
        let mut sh = ShenronServer { app };

        sh.run_on_address(config, addr).await?;

        Ok(())
    }
}

struct ShenronServer<A> {
    app: Arc<A>,
}

impl<A> server::Server for ShenronServer<A>
where
    A: App,
{
    type Handler = ShenronHandler<A>;

    fn new_client(&mut self, _addr: Option<SocketAddr>) -> Self::Handler {
        ShenronHandler {
            app: Arc::clone(&self.app),
            channels: Arc::new(Mutex::new(HashMap::new())),
            sessions: SessionMap::default(),
        }
    }
}

#[derive(Default)]
struct SessionMap {
    sessions: Arc<Mutex<HashMap<ChannelId, Arc<RwLock<Session>>>>>,
}

impl SessionMap {
    async fn get(&self, channel: ChannelId) -> Option<Arc<RwLock<Session>>> {
        let sessions = self.sessions.lock().await;

        sessions.get(&channel).cloned()
    }

    async fn insert(
        &self,
        channel: ChannelId,
        session: Arc<RwLock<Session>>,
    ) -> Option<Arc<RwLock<Session>>> {
        let mut sessions = self.sessions.lock().await;

        sessions.insert(channel, session)
    }
}

struct ShenronHandler<A> {
    app: Arc<A>,
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    sessions: SessionMap,
}

impl<A> server::Handler for ShenronHandler<A>
where
    A: App,
{
    type Error = crate::Error;

    async fn channel_open_session(
        &mut self,
        channel: Channel<russh::server::Msg>,
        _session: &mut RusshSession,
    ) -> Result<bool> {
        tracing::info!("Channel opened: {:?}", channel.id());

        self.channels.lock().await.insert(channel.id(), channel);

        Ok(true)
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        _public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<Auth> {
        tracing::info!("Pubkey auth attempt for user: {}", user);

        // TODO: impl publickey auth

        Ok(Auth::Accept)
    }

    async fn auth_password(&mut self, user: &str, _password: &str) -> Result<Auth> {
        tracing::info!("Password auth attempt for user: {}", user);

        // TODO: impl password auth

        Ok(Auth::Accept)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut RusshSession,
    ) -> Result<()> {
        tracing::info!(
            "PTY request on channel {:?}: {}x{} ({})",
            channel,
            col_width,
            row_height,
            term
        );

        let app_session = crate::Session::new();

        app_session
            .set_pty_size(PtySize {
                width: col_width,
                height: row_height,
                pixel_width: pix_width,
                pixel_height: pix_height,
            })
            .await;

        let app_session = Arc::new(RwLock::new(app_session));

        self.sessions
            .insert(channel, Arc::clone(&app_session))
            .await;

        let channels = Arc::clone(&self.channels);

        tokio::spawn(async move {
            let mut app_session = app_session.write().await;

            while let Some(data) = app_session.read().await {
                let channels = channels.lock().await;
                let channel_id = channel;

                if let Some(channel) = channels.get(&channel_id) {
                    if let Err(e) = channel.data(data.as_slice()).await {
                        tracing::error!("Failed to send data to channel {:?}: {}", channel_id, e);

                        break;
                    }
                } else {
                    break;
                }
            }
        });

        session.channel_success(channel)?;

        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut RusshSession,
    ) -> Result<()> {
        tracing::debug!("Data on channel: {:?}: {} bytes", channel, data.len());

        let Some(session) = self.sessions.get(channel).await else {
            return Err(crate::Error::Custom("Failed to send data".into()));
        };

        session.read().await.write(data).await?;

        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        _session: &mut RusshSession,
    ) -> Result<()> {
        tracing::debug!(
            "Window change on channel {:?}: {}x{}",
            channel,
            col_width,
            row_height
        );

        let size = PtySize {
            width: col_width,
            height: row_height,
            pixel_width: pix_width,
            pixel_height: pix_height,
        };

        let Some(session) = self.sessions.get(channel).await else {
            return Err(crate::Error::Custom("Reize failed".into()));
        };

        session.read().await.resize(size)?;

        Ok(())
    }
}
