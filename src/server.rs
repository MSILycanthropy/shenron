use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use russh::{
    Channel, ChannelId,
    keys::PrivateKey,
    server::{self, Auth, Config, Msg, Server as _, Session as RusshSession},
};
use tokio::sync::{Mutex, mpsc};

use crate::{
    App, Result,
    session::{PtySize, ResizeTx},
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
            inputs: Arc::new(Mutex::new(HashMap::new())),
            resizes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

struct ShenronHandler<A> {
    app: Arc<A>,
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    inputs: Arc<Mutex<HashMap<ChannelId, mpsc::Sender<Vec<u8>>>>>,
    resizes: Arc<Mutex<HashMap<ChannelId, ResizeTx>>>,
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

        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(32);
        let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(32);
        let (resize_tx, resize_rx) = mpsc::channel::<PtySize>(8);

        let app_session =
            crate::session::Session::new(session.handle(), channel, input_rx, output_tx, resize_rx);

        app_session
            .set_pty_size(PtySize {
                width: col_width,
                height: row_height,
                pixel_width: pix_width,
                pixel_height: pix_height,
            })
            .await;

        self.inputs.lock().await.insert(channel, input_tx);
        self.resizes.lock().await.insert(channel, resize_tx);

        let app = Arc::clone(&self.app);
        let channel_id = channel;

        tokio::spawn(async move {
            if let Err(e) = app.handle(app_session).await {
                tracing::error!("Application Error on Channel {:?}: {}", channel_id, e);
            }

            tracing::info!("Finished handling on channel {:?}", channel_id);
        });

        let channels = Arc::clone(&self.channels);

        tokio::spawn(async move {
            while let Some(data) = output_rx.recv().await {
                let channels = channels.lock().await;

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

        if let Some(input_tx) = self.inputs.lock().await.get(&channel)
            && let Err(e) = input_tx.send(data.to_vec()).await
        {
            tracing::error!(
                "Failed to send input to app on channel {:?}: {}",
                channel,
                e
            );
        }

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

        if let Some(resize_tx) = self.resizes.lock().await.get(&channel)
            && let Err(e) = resize_tx.send(size).await
        {
            tracing::error!(
                "Failed to send resize to app on channel {:?}: {}",
                channel,
                e
            );
        }
        Ok(())
    }
}
