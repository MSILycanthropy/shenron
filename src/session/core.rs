use std::{any::Any, collections::HashMap, net::SocketAddr};

use russh::{Channel, ChannelMsg, keys::PublicKey, server::Msg};

use crate::{Event, Extensions, PtySize, SessionKind};

pub struct Session {
    channel: Option<Channel<Msg>>,
    kind: SessionKind,
    pty: Option<(String, PtySize)>,
    user: String,
    public_key: Option<PublicKey>,
    env: HashMap<String, String>,
    extensions: Extensions,
    remote_addr: SocketAddr,
    exit_code: Option<u32>,
    exited: bool,
}

impl Session {
    #[expect(clippy::too_many_arguments, reason = "pub(crate), one call site")]
    pub(crate) const fn new(
        channel: Channel<Msg>,
        kind: SessionKind,
        pty: Option<(String, PtySize)>,
        user: String,
        public_key: Option<PublicKey>,
        env: HashMap<String, String>,
        extensions: Extensions,
        remote_addr: SocketAddr,
    ) -> Self {
        Self {
            channel: Some(channel),
            kind,
            pty,
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

                    if let Some((_, ref mut size)) = self.pty {
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

    /// Next chunk of input bytes, or `None` once the client is done sending.
    ///
    /// Non-input events arriving in between are consumed and discarded
    /// (resizes still update [`Session::pty`]). To observe resizes or
    /// signals as events, use [`Session::next`] instead.
    pub async fn input(&mut self) -> Option<Vec<u8>> {
        loop {
            match self.next().await? {
                Event::Input(data) => return Some(data),
                Event::Eof => return None,
                _ => {}
            }
        }
    }

    #[must_use]
    pub const fn kind(&self) -> &SessionKind {
        &self.kind
    }

    /// The PTY the client requested, if any. Orthogonal to [`kind`](Self::kind):
    /// `ssh -t host cmd` is an `Exec` session with a PTY.
    #[must_use]
    pub fn pty(&self) -> Option<(&str, PtySize)> {
        self.pty.as_ref().map(|(term, size)| (term.as_str(), *size))
    }

    /// The exec command as POSIX-parsed argv (Wish's `Command()`).
    ///
    /// `None` for non-exec sessions and for commands with invalid quoting —
    /// check [`raw_command`](Self::raw_command) to tell those apart.
    ///
    /// Execute the tokens directly (`Command::new(&argv[0]).args(&argv[1..])`);
    /// joining them back into one shell string reintroduces injection.
    #[must_use]
    pub fn command(&self) -> Option<Vec<String>> {
        match &self.kind {
            SessionKind::Exec { command } => shell_words::split(command).ok(),
            _ => None,
        }
    }

    /// The exec command exactly as the client sent it (Wish's `RawCommand()`).
    #[must_use]
    pub fn raw_command(&self) -> Option<&str> {
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

    /// The client's address, as reported by the accepted socket.
    ///
    /// Always the real peer address: connections whose address can't be read
    /// are rejected during auth and never reach a session.
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
    pub fn insert<T: Any + Clone + Send + Sync>(&mut self, value: T) {
        self.extensions.insert(value);
    }

    /// Mutably borrow a typed value attached during auth or by a middleware.
    #[must_use]
    pub fn get_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.extensions.get_mut::<T>()
    }

    /// Take the stored value of type `T` out of the session, if present.
    pub fn remove<T: Any>(&mut self) -> Option<T> {
        self.extensions.remove::<T>()
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

    /// Record a non-zero exit code to report when the handler returns.
    ///
    /// Calling this is only needed for failure codes: a handler returning
    /// `Ok(())` reports exit 0, and a handler returning `Err` reports exit 1.
    /// Returns an always-`Ok` `Result` so middleware can `return session.exit(1)`.
    /// To close the channel immediately instead, use [`abort`](Self::abort).
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

        self.finish(code).await
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
        self.pty.is_some() || matches!(self.kind, SessionKind::Shell)
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

    /// Send the exit status, EOF, and close the channel. Idempotent — once a
    /// session has finished, later calls (and a later natural handler return)
    /// no-op.
    pub(crate) async fn finish(&mut self, code: u32) -> crate::Result {
        if self.exited {
            return Ok(());
        }

        let Some(channel) = self.channel.as_ref() else {
            return Ok(());
        };

        self.exited = true;

        channel.exit_status(code).await?;
        channel.eof().await?;
        channel.close().await.map_err(crate::Error::Ssh)
    }
}
