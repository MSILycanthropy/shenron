/// What the client asked this session to run.
///
/// PTY allocation is orthogonal — any kind can carry one (`ssh -t host cmd`
/// is `Exec` with a PTY). See [`Session::pty`](crate::Session::pty).
#[derive(Clone)]
pub enum SessionKind {
    Shell,
    Exec { command: String },
    Subsystem { name: String },
}
