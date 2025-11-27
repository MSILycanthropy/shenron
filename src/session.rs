use std::{collections::HashMap, sync::Arc};

use tokio::sync::{RwLock, mpsc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub width: usize,
    pub height: usize,
    pub pixel_width: usize,
    pub pixel_height: usize,
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
pub(crate) type ResizeInput = mpsc::Receiver<PtySize>;
pub(crate) type ResizeOutput = mpsc::Sender<PtySize>;

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
    pub(crate) fn new(
        user: String,
        input: Input,
        output: Output,
        resize_input: ResizeInput,
        resize_output: ResizeOutput,
    ) -> Self {
        Self {
            user,
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

    pub async fn set_pty_size(&self, size: PtySize) {
        *self.pty_size.write().await = size;
    }

    pub async fn read(&mut self) -> Option<Vec<u8>> {
        self.input.recv().await
    }

    /// # Errors
    ///
    /// Will return `Err` if data failed to send
    pub async fn write(&self, data: &[u8]) -> crate::Result<()> {
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

    pub fn try_revc_resive(&mut self) -> Option<PtySize> {
        self.resize_input.try_recv().ok()
    }
}
