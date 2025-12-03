use shenron::{Event, Result, Server, Session, Signal};

async fn signal(mut session: Session) -> shenron::Result<Session> {
    session
        .write_str("Running... (send SIGINT to stop)\r\n")
        .await?;

    while let Some(event) = session.next().await {
        match event {
            Event::Input(data) => {
                if data.contains(&3) {
                    session.write_str("\r\nCtrl+C received\r\n").await?;
                    break;
                }
                session.write(&data).await?;
            }
            Event::Signal(sig) => match sig {
                Signal::INT => {
                    session.write_str("\r\nSIGINT received\r\n").await?;
                    break;
                }
                Signal::TERM => {
                    session.write_str("\r\nSIGTERM received\r\n").await?;
                    break;
                }
                other => {
                    session
                        .write_str(&format!("\r\nSignal: {other:?}\r\n"))
                        .await?;
                }
            },
            Event::Eof => break,
            Event::Resize(_) => {}
        }
    }

    session.exit(0)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting signal example on 0.0.0.0:2222");
    tracing::info!("Connect with: ssh -p 2222 localhost");

    Server::new().bind("0.0.0.0:2222").app(signal).serve().await
}
