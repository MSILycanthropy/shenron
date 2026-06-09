use crate::{Middleware, Next, Result, Session};

/// Allowlist of programs an exec request may run (Wish `accesscontrol` parity).
///
/// Compares the *program* — `argv[0]` of the POSIX-parsed command — exactly
/// against the list, so allowing `git` permits `git push` and `git pull`
/// alike. Sessions without an exec command (shells, subsystems) pass through
/// untouched. Commands that fail to parse are denied.
///
/// This is only a security boundary if the app executes the parsed argv
/// directly (`Command::new(&argv[0]).args(&argv[1..])`). Never hand
/// [`Session::raw_command`] to a shell: `allowed && anything` parses with
/// `argv[0] == "allowed"` and would sail through this check.
pub struct AccessControl {
    allowed: Vec<String>,
}

impl AccessControl {
    pub fn new(allowed: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allowed: allowed.into_iter().map(Into::into).collect(),
        }
    }

    fn is_allowed(&self, program: &str) -> bool {
        self.allowed.iter().any(|allowed| allowed == program)
    }
}

impl Middleware for AccessControl {
    async fn handle(&self, session: &'_ mut Session, next: Next<'_>) -> Result {
        if session.raw_command().is_none() {
            return next.run(session).await;
        }

        if let Some(argv) = session.command()
            && let Some(program) = argv.first()
            && self.is_allowed(program)
        {
            return next.run(session).await;
        }

        let raw = session.raw_command().unwrap_or_default();
        let message = format!("Command not allowed: {raw}\n");

        session.write_stderr_str(&message).await?;

        session.exit(1)
    }
}

#[cfg(test)]
mod tests {
    use super::AccessControl;

    #[test]
    fn only_listed_programs_are_allowed() {
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
