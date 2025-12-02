use crate::{PtySize, Signal};

#[derive(Debug)]
pub enum Event {
    Input(Vec<u8>),
    Resize(PtySize),
    Signal(Signal),
    Eof,
}
