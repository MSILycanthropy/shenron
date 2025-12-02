use std::collections::HashMap;

use russh::{Channel, server::Msg};
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub width: u32,
    pub height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

impl Default for PtySize {
    fn default() -> Self {
        Self {
            width: 80,
            height: 24,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

pub(crate) type Input = mpsc::Receiver<Vec<u8>>;
pub(crate) type ResizeTx = watch::Sender<PtySize>;
pub(crate) type ResizeRx = watch::Receiver<PtySize>;

pub struct Session {
    pub user: String,

    pub env: HashMap<String, String>,

    pub term: String,

    pub(crate) channel: Channel<Msg>,

    pub(crate) input: Input,

    pub(crate) resize_rx: ResizeRx,
}

impl Session {
    pub(crate) fn new(channel: Channel<Msg>, input: Input, resize_rx: ResizeRx) -> Self {
        Self {
            channel,
            user: "TODO: Figure this out".into(),
            env: HashMap::new(),
            term: String::from("xterm"),
            input,
            resize_rx,
        }
    }

    #[must_use]
    pub fn pty_size(&self) -> PtySize {
        *self.resize_rx.borrow()
    }

    pub async fn read(&mut self) -> Option<Vec<u8>> {
        self.input.recv().await
    }

    /// # Errors
    ///
    /// Will return `Err` if data failed to send
    pub async fn write(&self, data: &[u8]) -> crate::Result<()> {
        tracing::debug!("Attemping to write {:?} to ouput", &data);

        self.channel
            .data(data)
            .await
            .map_err(|_| crate::Error::Custom("Failed to send data".into()))
    }

    /// # Errors
    ///
    /// Will return `Err` if data failed to send
    pub async fn write_str(&self, s: &str) -> crate::Result<()> {
        self.write(s.as_bytes()).await
    }

    pub async fn wait_for_resize(&mut self) -> Option<PtySize> {
        self.resize_rx.changed().await.ok()?;

        Some(self.pty_size())
    }

    pub fn has_resized(&mut self) -> bool {
        self.resize_rx.has_changed().unwrap_or(false)
    }

    /// # Errors
    ///
    /// Will return `Err` if russh fails to close the session
    pub async fn end(&self) -> crate::Result<()> {
        self.channel
            .close()
            .await
            .map_err(|_| crate::Error::Custom("Failed to end session".into()))
    }
}
