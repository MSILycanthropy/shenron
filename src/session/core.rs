use std::{collections::HashMap, net::SocketAddr};

use russh::{Channel, ChannelMsg, server::Msg};

use crate::{Event, PtySize, SessionKind};

pub struct Session {
    channel: Channel<Msg>,
    kind: SessionKind,
    user: String,
    env: HashMap<String, String>,
    remote_addr: SocketAddr,
    exit_code: Option<u32>,
}

impl Session {
    pub(crate) const fn new(
        channel: Channel<Msg>,
        kind: SessionKind,
        user: String,
        env: HashMap<String, String>,
        remote_addr: SocketAddr,
    ) -> Self {
        Self {
            channel,
            kind,
            user,
            env,
            remote_addr,
            exit_code: None,
        }
    }

    pub async fn next(&mut self) -> Option<Event> {
        loop {
            let event = self.channel_mut().wait().await?;

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

    #[must_use]
    pub const fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    #[must_use]
    pub const fn env(&self) -> &HashMap<String, String> {
        &self.env
    }

    /// Write data to the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write(&self, data: &[u8]) -> crate::Result<()> {
        self.channel().data(data).await.map_err(crate::Error::Ssh)
    }

    /// Write a string to the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write_str(&self, s: &str) -> crate::Result<()> {
        self.write(s.as_bytes()).await
    }

    /// Write to stderr on the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write_stderr(&self, data: &[u8]) -> crate::Result<()> {
        self.channel()
            .extended_data(1, data)
            .await
            .map_err(crate::Error::Ssh)
    }

    /// Write a string to stderr on the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write_stderr_str(&self, s: &str) -> crate::Result<()> {
        self.write_stderr(s.as_bytes()).await
    }

    #[allow(clippy::missing_errors_doc)]
    pub const fn exit(mut self, code: u32) -> crate::Result<Self> {
        self.exit_code = Some(code);

        Ok(self)
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
    pub async fn abort(mut self, code: u32) -> crate::Result<Self> {
        self.exit_code = Some(code);

        self.do_exit().await?;

        Ok(self)
    }

    #[must_use]
    pub const fn will_exit(&self) -> bool {
        self.exit_code.is_some()
    }

    #[must_use]
    pub const fn is_interactive(&self) -> bool {
        matches!(self.kind, SessionKind::Pty { .. } | SessionKind::Shell)
    }

    #[must_use]
    pub const fn channel(&self) -> &Channel<Msg> {
        &self.channel
    }

    pub const fn channel_mut(&mut self) -> &mut Channel<Msg> {
        &mut self.channel
    }

    /// WARNING: A call to this method bricks the session, use with the UTMOST caution.
    #[cfg(feature = "sftp")]
    #[allow(unsafe_code, invalid_value)]
    pub(crate) const fn unsafe_take_channel(&mut self) -> Channel<Msg> {
        std::mem::replace(&mut self.channel, unsafe { std::mem::zeroed() })
    }

    pub(crate) async fn do_exit(&self) -> crate::Result<()> {
        let Some(exit_code) = self.exit_code else {
            return Ok(());
        };

        self.channel().exit_status(exit_code).await?;
        self.channel().eof().await?;
        self.channel().close().await.map_err(crate::Error::Ssh)
    }
}
