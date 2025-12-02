use std::{collections::HashMap, sync::Arc};

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

pub(crate) type Input = mpsc::Sender<Vec<u8>>;
pub(crate) type Output = mpsc::Receiver<Vec<u8>>;
pub(crate) type ResizeInput = mpsc::Sender<PtySize>;
pub(crate) type ResizeOutput = mpsc::Receiver<PtySize>;

pub struct Session {
    pub user: String,

    pty_size: Arc<RwLock<PtySize>>,

    pub env: HashMap<String, String>,

    pub term: String,

    pub(crate) input: Input,

    pub(crate) output: Output,

    pub(crate) resize_input: ResizeInput,

    pub(crate) resize_output: ResizeOutput,
}

impl Session {
    pub(crate) fn new() -> Self {
        let (input, output) = mpsc::channel::<Vec<u8>>(32);
        let (resize_input, resize_output) = mpsc::channel::<PtySize>(8);

        Self {
            user: "TODO: Figure this out".into(),
            pty_size: Arc::new(RwLock::new(PtySize::default())),
            env: HashMap::new(),
            term: String::from("xterm"),
            input,
            output,
            resize_input,
            resize_output,
        }
    }

    pub async fn pty_size(&self) -> PtySize {
        *self.pty_size.read().await
    }

    pub(crate) async fn set_pty_size(&self, size: PtySize) {
        *self.pty_size.write().await = size;
    }

    pub(crate) fn resize(&self, size: PtySize) -> crate::Result<()> {
        self.resize_input
            .try_send(size)
            .map_err(|_| crate::Error::Custom("Failed to resize".into()))
    }

    pub async fn read(&mut self) -> Option<Vec<u8>> {
        self.output.recv().await
    }

    /// # Errors
    ///
    /// Will return `Err` if data failed to send
    pub async fn write(&self, data: &[u8]) -> crate::Result<()> {
        self.input
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

    pub fn try_revc_resive(&mut self) -> Option<PtySize> {
        self.resize_output.try_recv().ok()
    }
}
