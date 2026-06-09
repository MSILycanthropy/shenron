use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use russh::{
    Channel, ChannelId,
    keys::PublicKey,
    server::{Auth, Msg, Session as RusshSession},
};

use crate::{
    Auth as AuthOutcome, Extensions, PtySize, Session, SessionKind, auth::AuthConfig,
    middleware::ErasedHandler,
};

/// Concurrent session channels allowed per connection (pending + running).
/// Matches OpenSSH's `MaxSessions` default.
const MAX_SESSIONS: usize = 10;

/// Client-controlled env vars are stored per channel; cap them so a hostile
/// client can't grow memory without bound. Requests beyond the cap are
/// silently dropped, like OpenSSH's `AcceptEnv` rejections.
const MAX_ENV_VARS: usize = 128;

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
            pending: HashMap::new(),
            running: Arc::new(AtomicUsize::new(0)),
            user: None,
            public_key: None,
            auth: Arc::clone(&self.auth),
            extensions: Extensions::default(),
            banner: self.banner.clone(),
        }
    }
}

/// Holds a slot in the connection's session count; releases it on drop, so
/// the count stays correct even if the handler task panics.
struct RunningGuard(Arc<AtomicUsize>);

impl RunningGuard {
    fn new(count: Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::Relaxed);

        Self(count)
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A session channel that has been opened but not yet started: `env` and
/// `pty-req` requests accumulate here until shell/exec/subsystem arrives.
struct PendingChannel {
    channel: Channel<Msg>,
    env: HashMap<String, String>,
    pty: Option<(String, PtySize)>,
}

pub(crate) struct ShenronHandler {
    handler: Arc<dyn ErasedHandler>,
    remote_addr: SocketAddr,
    pending: HashMap<ChannelId, PendingChannel>,
    running: Arc<AtomicUsize>,
    user: Option<String>,
    public_key: Option<PublicKey>,
    auth: Arc<AuthConfig>,
    extensions: Extensions,
    banner: Option<String>,
}

impl ShenronHandler {
    /// Record the user on success, or build a rejection that only advertises
    /// the auth methods this server actually has configured.
    fn finish_auth(&mut self, user: &str, accepted: bool) -> Auth {
        if accepted {
            self.user = Some(user.to_string());

            return Auth::Accept;
        }

        Auth::Reject {
            proceed_with_methods: Some(self.auth.methods()),
            partial_success: false,
        }
    }

    /// Pull the pending channel for `id` and build the app session from its
    /// accumulated state plus a snapshot of the connection's auth data.
    fn start_session(&mut self, id: ChannelId, kind: SessionKind) -> crate::Result<Session> {
        let pending = self
            .pending
            .remove(&id)
            .ok_or_else(|| crate::Error::Protocol("No channel available".into()))?;

        Ok(Session::new(
            pending.channel,
            kind,
            pending.pty,
            self.user.clone().unwrap_or_else(|| "unknown".into()),
            self.public_key.clone(),
            pending.env,
            self.extensions.clone(),
            self.remote_addr,
        ))
    }

    fn run_handler(&self, mut session: Session) {
        let handler = Arc::clone(&self.handler);
        let running = RunningGuard::new(Arc::clone(&self.running));

        tokio::spawn(async move {
            let _running = running;

            let code = match handler.call(&mut session).await {
                Ok(()) => session.exit_code().unwrap_or(0),
                Err(e) => {
                    tracing::error!("Handler error: {e}");

                    1
                }
            };

            if let Err(e) = session.finish(code).await {
                tracing::debug!("failed to close session channel: {e}");
            }
        });
    }
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
        if self.pending.len() + self.running.load(Ordering::Relaxed) >= MAX_SESSIONS {
            return Ok(false);
        }

        self.pending.insert(
            channel.id(),
            PendingChannel {
                channel,
                env: HashMap::new(),
                pty: None,
            },
        );

        Ok(true)
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut RusshSession,
    ) -> crate::Result<()> {
        // A pending channel closed without starting a session; free its slot.
        self.pending.remove(&channel);

        Ok(())
    }

    async fn auth_publickey(&mut self, user: &str, public_key: &PublicKey) -> crate::Result<Auth> {
        let outcome: AuthOutcome = if let Some(ref handler) = self.auth.pubkey {
            handler.verify(user, public_key).await
        } else {
            self.auth.is_empty().into()
        };

        let accepted = outcome.accepted();

        if accepted {
            self.public_key = Some(public_key.clone());
            self.extensions.merge(outcome.into_extensions());
        }

        Ok(self.finish_auth(user, accepted))
    }

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> crate::Result<russh::server::Auth> {
        let outcome: AuthOutcome = if let Some(ref handler) = self.auth.password {
            handler.verify(user, password).await
        } else {
            self.auth.is_empty().into()
        };

        let accepted = outcome.accepted();

        if accepted {
            self.extensions.merge(outcome.into_extensions());
        }

        Ok(self.finish_auth(user, accepted))
    }

    async fn env_request(
        &mut self,
        channel: russh::ChannelId,
        variable_name: &str,
        variable_value: &str,
        _session: &mut RusshSession,
    ) -> crate::Result<()> {
        let Some(pending) = self.pending.get_mut(&channel) else {
            return Ok(());
        };

        if pending.env.len() >= MAX_ENV_VARS {
            tracing::debug!("env var limit reached, dropping {variable_name}");

            return Ok(());
        }

        pending
            .env
            .insert(variable_name.to_string(), variable_value.to_string());

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
        let Some(pending) = self.pending.get_mut(&channel_id) else {
            session.channel_failure(channel_id)?;

            return Ok(());
        };

        pending.pty = Some((
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

    async fn exec_request(
        &mut self,
        channel_id: russh::ChannelId,
        data: &[u8],
        session: &mut RusshSession,
    ) -> crate::Result<()> {
        let command = String::from_utf8_lossy(data).to_string();
        let app_session = self.start_session(channel_id, SessionKind::Exec { command })?;

        session.channel_success(channel_id)?;

        self.run_handler(app_session);

        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel_id: russh::ChannelId,
        session: &mut RusshSession,
    ) -> crate::Result<()> {
        let app_session = self.start_session(channel_id, SessionKind::Shell)?;

        session.channel_success(channel_id)?;

        self.run_handler(app_session);

        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: russh::ChannelId,
        name: &str,
        session: &mut RusshSession,
    ) -> crate::Result<()> {
        let kind = SessionKind::Subsystem {
            name: name.to_string(),
        };
        let app_session = self.start_session(channel_id, kind)?;

        session.channel_success(channel_id)?;

        self.run_handler(app_session);

        Ok(())
    }
}
