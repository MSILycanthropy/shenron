pub mod core;
mod event;
mod key;
pub(crate) mod writer;

pub use core::Tui;
pub use event::Event;
pub use key::parse_key_event;
