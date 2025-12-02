use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use russh::{
    Channel, MethodKind,
    keys::PublicKey,
    server::{Auth, Msg, Session as RusshSession},
};

use crate::{PtySize, Session, SessionKind, auth::AuthConfig, middleware::ErasedHandler};

pub(crate) struct ShenronServer {
    pub(crate) handler: Arc<dyn ErasedHandler>,
    pub(crate) auth: Arc<AuthConfig>,
    pub(crate) banner: Option<String>,
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
            banner: self.banner.clone(),
        }
    }
}

pub(crate) struct ShenronHandler {
    handler: Arc<dyn ErasedHandler>,
    remote_addr: SocketAddr,
    channel: Option<Channel<Msg>>,
    user: Option<String>,
    auth: Arc<AuthConfig>,
    env: HashMap<String, String>,
    pty: Option<(String, PtySize)>,
    banner: Option<String>,
}

impl russh::server::Handler for ShenronHandler {
    type Error = crate::Error;

    async fn authentication_banner(&mut self) -> crate::Result<Option<String>> {
        Ok(self.banner.clone())
    }

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

        run_handler(handler, app_session);

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

        run_handler(handler, app_session);

        session.channel_success(channel_id)?;

        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: russh::ChannelId,
        name: &str,
        session: &mut RusshSession,
    ) -> crate::Result<()> {
        let channel = self
            .channel
            .take()
            .ok_or_else(|| crate::Error::Protocol("No channel available".into()))?;

        let user = self.user.clone().unwrap_or_else(|| "unknown".into());

        let app_session = crate::Session::new(
            channel,
            SessionKind::Subsystem {
                name: name.to_string(),
            },
            user,
            std::mem::take(&mut self.env),
            self.remote_addr,
        );

        let handler = Arc::clone(&self.handler);

        run_handler(handler, app_session);

        session.channel_success(channel_id)?;

        Ok(())
    }
}

fn run_handler(handler: Arc<dyn ErasedHandler>, session: Session) {
    tokio::spawn(async move {
        match handler.call(session).await {
            Ok(session) => {
                let _ = session.abort().await;
            }
            Err(e) => {
                tracing::error!("Handler error: {}", e);
            }
        }
    });
}
