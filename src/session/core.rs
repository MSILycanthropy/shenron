use std::{any::Any, collections::HashMap, net::SocketAddr};

use russh::{Channel, ChannelMsg, keys::PublicKey, server::Msg};

use crate::{Event, Extensions, PtySize, SessionKind};

pub struct Session {
    channel: Option<Channel<Msg>>,
    kind: SessionKind,
    user: String,
    public_key: Option<PublicKey>,
    env: HashMap<String, String>,
    extensions: Extensions,
    remote_addr: SocketAddr,
    exit_code: Option<u32>,
    exited: bool,
}

impl Session {
    pub(crate) const fn new(
        channel: Channel<Msg>,
        kind: SessionKind,
        user: String,
        public_key: Option<PublicKey>,
        env: HashMap<String, String>,
        extensions: Extensions,
        remote_addr: SocketAddr,
    ) -> Self {
        Self {
            channel: Some(channel),
            kind,
            user,
            public_key,
            env,
            extensions,
            remote_addr,
            exit_code: None,
            exited: false,
        }
    }

    pub async fn next(&mut self) -> Option<Event> {
        loop {
            let event = self.channel.as_mut()?.wait().await?;

            match event {
                ChannelMsg::Data { data } => return Some(Event::Input(data.to_vec())),
                ChannelMsg::WindowChange {
                    col_width,
                    row_height,
                    pix_width,
                    pix_height,
                } => {
                    let new_size = PtySize {
                        width: col_width,
                        height: row_height,
                        pixel_width: pix_width,
                        pixel_height: pix_height,
                    };

                    if let SessionKind::Pty { ref mut size, .. } = self.kind {
                        *size = new_size;
                    }

                    return Some(Event::Resize(new_size));
                }
                ChannelMsg::Signal { signal } => return Some(Event::Signal(signal)),
                ChannelMsg::Eof => return Some(Event::Eof),

                // Skip protocol messages
                _ => {}
            }
        }
    }

    pub async fn input(&mut self) -> Option<Vec<u8>> {
        match self.next().await? {
            Event::Input(data) => Some(data),
            _ => None,
        }
    }

    #[must_use]
    pub fn kind(&self) -> SessionKind {
        self.kind.clone()
    }

    #[must_use]
    pub fn pty(&self) -> Option<(&str, PtySize)> {
        match &self.kind {
            SessionKind::Pty { term, size } => Some((term, *size)),
            _ => None,
        }
    }

    #[must_use]
    pub fn command(&self) -> Option<&str> {
        match &self.kind {
            SessionKind::Exec { command } => Some(command),
            _ => None,
        }
    }

    #[must_use]
    pub fn subsystem(&self) -> Option<&str> {
        match &self.kind {
            SessionKind::Subsystem { name } => Some(name),
            _ => None,
        }
    }

    #[must_use]
    pub fn pty_size(&self) -> Option<PtySize> {
        let pty = self.pty()?;

        Some(pty.1)
    }

    #[must_use]
    pub fn term(&self) -> Option<&str> {
        let pty = self.pty()?;

        Some(pty.0)
    }

    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The public key the session authenticated with, if any.
    ///
    /// Returns `None` when the user authenticated by password or when no auth
    /// handler was configured.
    #[must_use]
    pub const fn public_key(&self) -> Option<&PublicKey> {
        self.public_key.as_ref()
    }

    #[must_use]
    pub const fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    #[must_use]
    pub const fn env(&self) -> &HashMap<String, String> {
        &self.env
    }

    /// Borrow a typed value attached during auth or by a middleware.
    ///
    /// Returns `None` if nothing of type `T` was stored. See
    /// [`Auth::with`](crate::Auth::with) and [`insert`](Self::insert).
    #[must_use]
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.extensions.get::<T>()
    }

    /// Attach a typed value, replacing any existing value of the same type.
    pub fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        self.extensions.insert(value);
    }

    /// Write data to the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write(&self, data: &[u8]) -> crate::Result {
        self.channel()?.data(data).await.map_err(crate::Error::Ssh)
    }

    /// Write a string to the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write_str(&self, s: &str) -> crate::Result {
        self.write(s.as_bytes()).await
    }

    /// Write to stderr on the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write_stderr(&self, data: &[u8]) -> crate::Result {
        self.channel()?
            .extended_data(1, data)
            .await
            .map_err(crate::Error::Ssh)
    }

    /// Write a string to stderr on the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write_stderr_str(&self, s: &str) -> crate::Result {
        self.write_stderr(s.as_bytes()).await
    }

    #[allow(clippy::missing_errors_doc)]
    pub const fn exit(&mut self, code: u32) -> crate::Result {
        self.exit_code = Some(code);

        Ok(())
    }

    #[must_use]
    pub const fn exit_code(&self) -> Option<u32> {
        self.exit_code
    }

    /// Set exit code and exit immediately
    ///
    /// # Errors
    ///
    /// Returns `Err` if
    ///   - Setting exit status fails
    ///   - Sending the eof message fails
    ///   - Closing the channel fails
    pub async fn abort(&mut self, code: u32) -> crate::Result {
        self.exit_code = Some(code);

        self.do_exit().await
    }

    /// Begin an own-the-loop session: merges SSH input with application
    /// messages pushed through [`Events::sender`](crate::events::Events::sender).
    ///
    /// Borrows the session; drop the returned [`Events`](crate::events::Events)
    /// to use the session again.
    pub fn events<M>(&mut self) -> crate::events::Events<'_, M> {
        crate::events::Events::new(self)
    }

    /// Begin a terminal UI session driven by `ratatui`.
    ///
    /// Borrows the session; drop the returned [`Tui`](crate::tui::Tui) (via
    /// [`close`](crate::tui::Tui::close)) to use the session again.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the client did not request a PTY.
    #[cfg(feature = "ratatui")]
    pub fn tui<M>(&mut self) -> crate::Result<crate::tui::Tui<'_, M>> {
        crate::tui::Tui::new(self)
    }

    #[must_use]
    pub const fn will_exit(&self) -> bool {
        self.exit_code.is_some()
    }

    #[must_use]
    pub const fn is_interactive(&self) -> bool {
        matches!(self.kind, SessionKind::Pty { .. } | SessionKind::Shell)
    }

    fn channel(&self) -> crate::Result<&Channel<Msg>> {
        self.channel
            .as_ref()
            .ok_or_else(|| crate::Error::Protocol("channel unavailable".into()))
    }

    /// Take ownership of the underlying channel, leaving the session without one.
    ///
    /// Subsequent reads/writes on the session will fail. Used by subsystems
    /// like SFTP that need to drive the raw channel themselves.
    #[cfg(feature = "sftp")]
    pub(crate) const fn take_channel(&mut self) -> Option<Channel<Msg>> {
        self.channel.take()
    }

    pub(crate) async fn do_exit(&mut self) -> crate::Result {
        if self.exited {
            return Ok(());
        }

        let Some(exit_code) = self.exit_code else {
            return Ok(());
        };

        let Some(channel) = self.channel.as_ref() else {
            return Ok(());
        };

        self.exited = true;

        channel.exit_status(exit_code).await?;
        channel.eof().await?;
        channel.close().await.map_err(crate::Error::Ssh)
    }
}
