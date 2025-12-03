pub mod auth;
mod error;
mod handler;
pub mod middleware;
pub mod server;
mod session;
#[cfg(feature = "ratatui")]
pub mod tui;

use std::pin::Pin;

pub use error::{Error, Result};
pub use handler::Handler;
pub use middleware::{Middleware, Next};
pub use server::Server;
pub use session::{Event, PtySize, Session, SessionKind, Signal};

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
