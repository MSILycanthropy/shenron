use std::{collections::HashMap, net::SocketAddr};

use russh::{Channel, ChannelMsg, server::Msg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub width: u32,
    pub height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[derive(Debug)]
pub enum Event {
    Input(Vec<u8>),
    Resize(PtySize),
    Eof,
}

pub struct Session {
    channel: Channel<Msg>,
    pty_size: PtySize,
    user: String,
    term: String,
    env: HashMap<String, String>,
    remote_addr: SocketAddr,
}

impl Session {
    pub(crate) const fn new(
        channel: Channel<Msg>,
        pty_size: PtySize,
        user: String,
        term: String,
        env: HashMap<String, String>,
        remote_addr: SocketAddr,
    ) -> Self {
        Self {
            channel,
            pty_size,
            user,
            term,
            env,
            remote_addr,
        }
    }

    pub async fn next(&mut self) -> Option<Event> {
        loop {
            let event = self.channel.wait().await?;

            match event {
                ChannelMsg::Data { data } => return Some(Event::Input(data.to_vec())),
                ChannelMsg::WindowChange {
                    col_width,
                    row_height,
                    pix_width,
                    pix_height,
                } => {
                    self.pty_size = PtySize {
                        width: col_width,
                        height: row_height,
                        pixel_width: pix_width,
                        pixel_height: pix_height,
                    };

                    return Some(Event::Resize(self.pty_size));
                }
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
    pub const fn pty_size(&self) -> PtySize {
        self.pty_size
    }

    #[must_use]
    pub fn term(&self) -> &str {
        &self.term
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
        self.channel.data(data).await.map_err(crate::Error::Ssh)
    }

    /// Write a string to the channel
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send
    pub async fn write_str(&self, s: &str) -> crate::Result<()> {
        self.write(s.as_bytes()).await
    }

    /// Close the session
    ///
    /// # Errors
    ///
    /// Returns `Err` if closing fails
    pub async fn close(&self) -> crate::Result<()> {
        self.channel.close().await.map_err(crate::Error::Ssh)
    }

    #[must_use]
    pub const fn channel(&self) -> &Channel<Msg> {
        &self.channel
    }
}
