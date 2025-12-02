use crate::PtySize;

#[derive(Clone)]
pub enum SessionKind {
    Pty { term: String, size: PtySize },
    Exec { command: String },
    Shell,
}
