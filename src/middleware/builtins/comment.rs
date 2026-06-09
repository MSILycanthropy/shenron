use crate::{Middleware, Next, Result, Session};

/// Middleware to print a message at the end of a session
pub struct Comment(pub String);

impl Middleware for Comment {
    async fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>) -> Result {
        next.run(session).await?;
        session.write_str(&self.0).await?;
        Ok(())
    }
}
