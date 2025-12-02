use crate::{Middleware, Next, Result, Session};

#[derive(Clone)]
pub struct Comment(pub String);

impl Middleware for Comment {
    async fn handle(&self, session: Session, next: Next) -> Result<Session> {
        let session = next.run(session).await?;

        session.write_str(&self.0).await?;

        Ok(session)
    }
}
