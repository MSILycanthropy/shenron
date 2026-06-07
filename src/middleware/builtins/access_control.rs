use crate::{Middleware, Next, Result, Session};

#[derive(Clone)]
pub struct AccessControl {
    allowed: Vec<String>,
}

impl AccessControl {
    pub fn new(allowed: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allowed: allowed.into_iter().map(Into::into).collect(),
        }
    }

    fn is_allowed(&self, cmd: &str) -> bool {
        self.allowed.iter().any(|allowed| allowed == cmd)
    }
}

impl Middleware for AccessControl {
    async fn handle(&self, session: Session, next: Next) -> Result<Session> {
        let Some(command) = session.command() else {
            return next.run(session).await;
        };

        let cmd = command.split_whitespace().next().unwrap_or("");

        if self.is_allowed(cmd) {
            return next.run(session).await;
        }

        session
            .write_stderr_str(&format!("Command not allowed: {cmd}\n"))
            .await?;

        session.exit(1)
    }
}

#[cfg(test)]
mod tests {
    use super::AccessControl;

    #[test]
    fn only_listed_commands_are_allowed() {
        let ac = AccessControl::new(["ls", "cat"]);

        assert!(ac.is_allowed("ls"));
        assert!(ac.is_allowed("cat"));
        assert!(!ac.is_allowed("rm"));
        assert!(!ac.is_allowed(""));
    }

    #[test]
    fn matching_is_exact_not_prefix() {
        let ac = AccessControl::new(["ls"]);

        assert!(!ac.is_allowed("lsof"));
        assert!(!ac.is_allowed("ls -la"));
    }
}
