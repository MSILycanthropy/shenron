#![feature(async_fn_traits, unboxed_closures)]

pub mod auth;
mod error;
pub mod events;
mod exit;
pub mod middleware;
pub mod server;
mod session;
#[cfg(feature = "ratatui")]
pub mod tui;

/// SFTP server support. Requires the `sftp` feature.
#[cfg(feature = "sftp")]
pub use middleware::builtins::sftp;

use std::pin::Pin;

pub use auth::Auth;
pub use error::{Error, Result};
pub use events::Events;
pub use exit::{Exit, IntoExit};
pub use middleware::{Middleware, Next, terminal};
pub use russh::keys::{Algorithm, EcdsaCurve};
pub use server::{HostKeyOptions, Server};
pub use session::{Event, Extensions, PtySize, Session, SessionKind, Signal};

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
