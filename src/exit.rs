/// What a session reports when the handler chain returns.
///
/// Produced from handler return values via [`IntoExit`]; consumed by the
/// server, which sends the exit status and closes the channel.
#[derive(Debug)]
pub enum Exit {
    /// Exit with this status code.
    Code(u32),
    /// Exit with status 1; the error is logged by the server and visible to
    /// middleware inspecting the chain's result.
    Error(crate::Error),
}

impl Exit {
    /// The status code this exit reports to the client.
    #[must_use]
    pub const fn code(&self) -> u32 {
        match self {
            Self::Code(code) => *code,
            Self::Error(_) => 1,
        }
    }
}

/// Conversion from a handler's return value into an [`Exit`], in the spirit
/// of [`std::process::Termination`] and axum's `IntoResponse`.
///
/// Handlers and middleware may return:
/// - `()` — exit 0
/// - `u32` — exit with that code
/// - [`Exit`] — passed through unchanged
/// - `Result<T, E>` where `T: IntoExit`, `E: Into<Error>` — `Ok` defers to
///   `T`, `Err` becomes [`Exit::Error`] (exit 1, logged)
pub trait IntoExit {
    fn into_exit(self) -> Exit;
}

impl IntoExit for () {
    fn into_exit(self) -> Exit {
        Exit::Code(0)
    }
}

impl IntoExit for u32 {
    fn into_exit(self) -> Exit {
        Exit::Code(self)
    }
}

impl IntoExit for Exit {
    fn into_exit(self) -> Exit {
        self
    }
}

impl<T: IntoExit, E: Into<crate::Error>> IntoExit for Result<T, E> {
    fn into_exit(self) -> Exit {
        match self {
            Ok(value) => value.into_exit(),
            Err(e) => Exit::Error(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_success() {
        assert_eq!(().into_exit().code(), 0);
    }

    #[test]
    fn code_passes_through() {
        assert_eq!(2u32.into_exit().code(), 2);
    }

    #[test]
    fn ok_defers_to_inner() {
        let ok: crate::Result<u32> = Ok(3);

        assert_eq!(ok.into_exit().code(), 3);
    }

    #[test]
    fn err_is_exit_one() {
        let err: crate::Result = Err(crate::Error::Protocol("boom".into()));
        let exit = err.into_exit();

        assert_eq!(exit.code(), 1);
        assert!(matches!(exit, Exit::Error(_)));
    }
}
