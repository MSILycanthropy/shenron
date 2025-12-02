use std::{collections::HashMap, sync::Arc};

use russh::{ChannelId, server::Handle};
use tokio::sync::{RwLock, mpsc};

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
pub(crate) type Output = mpsc::Sender<Vec<u8>>;
pub(crate) type ResizeTx = mpsc::Sender<PtySize>;
pub(crate) type ResizeRx = mpsc::Receiver<PtySize>;

pub struct Session {
    pub id: ChannelId,

    pub user: String,

    pty_size: Arc<RwLock<PtySize>>,

    pub env: HashMap<String, String>,

    pub term: String,

    pub(crate) handle: Handle,

    pub(crate) input: Input,

    pub(crate) output: Output,

    pub(crate) resize_rx: ResizeRx,
}

impl Session {
    pub(crate) fn new(
        handle: Handle,
        id: ChannelId,
        input: Input,
        output: Output,
        resize_rx: ResizeRx,
    ) -> Self {
        Self {
            id,
            user: "TODO: Figure this out".into(),
            pty_size: Arc::new(RwLock::new(PtySize::default())),
            env: HashMap::new(),
            term: String::from("xterm"),
            input,
            output,
            resize_rx,
            handle,
        }
    }

    pub async fn pty_size(&self) -> PtySize {
        *self.pty_size.read().await
    }

    pub(crate) async fn set_pty_size(&self, size: PtySize) {
        *self.pty_size.write().await = size;
    }

    pub async fn read(&mut self) -> Option<Vec<u8>> {
        self.input.recv().await
    }

    /// # Errors
    ///
    /// Will return `Err` if data failed to send
    pub async fn write(&self, data: &[u8]) -> crate::Result<()> {
        tracing::debug!("Attemping to write {:?} to ouput", &data);

        self.output
            .send(data.to_vec())
            .await
            .map_err(|_| crate::Error::Custom("Failed to send data".into()))
    }

    /// # Errors
    ///
    /// Will return `Err` if data failed to send
    pub async fn write_str(&self, s: &str) -> crate::Result<()> {
        self.write(s.as_bytes()).await
    }

    pub fn try_recv_resize(&mut self) -> Option<PtySize> {
        self.resize_rx.try_recv().ok()
    }

    /// # Errors
    ///
    /// Will return `Err` if russh fails to close the session
    pub async fn end(&self) -> crate::Result<()> {
        self.handle
            .close(self.id)
            .await
            .map_err(|()| crate::Error::Custom("Failed to end session".into()))
    }
}
