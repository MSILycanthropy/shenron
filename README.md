# Shrenon

[wish]: https://github.com/charmbracelet/wish
[ratatui]: https://github.com/ratatui/ratatui
[russh]: https://github.com/Eugeny/russh
[contribute]: https://github.com/MSILycanthropy/Shenron/contribute

🐉 Come forth, Shenron! Grant my wish — Elegant SSH apps in Rust! 🐉

SSH is a great platform for building applications that are accessible remotely.

- No need for HTTPs certificates like the web
- easy user identification with keys
- access from _any_ terminal

Many protocols like SFTP and Git work over SSH, and you can even render TUIs over SSH
to provide a UI for users.

Shenron is an SSH server with sensible defaults and a collection of middleware to make
building SSH applications painless. Shenron is inspired heavily by [charmbracelet/wish][wish].

Shenron is built on [Eugeny/russh][russh], so OpenSSH is not needed at all. Thus, there is no risk of leaking a shell, because there is no behavior that does so in Shenron.

## What is an SSH app?

Typically when connecting via SSH, the server is running something like `openssh-server` to get a shell.

But SSH is much more than that. It's a full cryptographic network protocol like HTTP![^1]

[^1]: https://en.wikipedia.org/wiki/Secure_Shell

That means we can write custom SSH servers that do whatever we want, without using `openssh-server` at all. We can do much much more than just provide a shell, like serve
a TUI to sell coffee.

Shenron is a library that makes writing this type of app in Rust just a wish away.

## Creating an App

Your app is just an async function that takes a `Session` and returns it when done:

```rust
async fn my_app(mut session: Session) -> shenron::Result<Session> {
    session.write_str("Hello from Shenron!\r\n").await?;
    session.exit(0)
}
```

Then, all you need to do is create a server for your app, Shenron ships an always authenticating default SSH server with automatic key generation.

```rust
Server::default()
    .bind("0.0.0.0:2222")
    .app(my_app)
    .serve()
    .await
```

## Examples

There are examples for a standalone [Ratatui app](examples/ratatui) and others in the [examples](examples) folder.

## Middleware

Shenron middleware works like middleware in most HTTP frameworks. Each middleware wraps the next, letting you handle sessions before and after passing them down the chain.

```rust
async fn my_middleware(session: Session, next: Next) -> Result {
    // do stuff before
    let session = next.run(session).await?;
    // do stuff after
    session.exit(0)
}
```

Middleware is composed outside-in — the first middleware you add is the outermost layer,
meaning it sees the session first and the result last.

```rust
Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(logging)        // 1st: sees session first, result last
    .with(activeterm)     // 2nd: runs inside logging
    .app(my_app)          // innermost: your application
    .serve()
    .await
```

## Built-In Middleware

Shenron ships with a collection of middleware to handle common tasks.

### Ratatui

The `ratatui` middleware makes it easy to serve [ratatui][ratatui] TUIs over SSH.
Each session gets its own app instance with window resize handled automatically.

```rust
use shenron::tui::{App, Ratatui};

#[derive(Clone)]
struct MyApp { /* ... */ }

impl App for MyApp {
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // return false to exit
        true
    }

    fn draw(&self, frame: &mut Frame) {
        // draw your UI
    }
}

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .app(Ratatui { app: MyApp::new() })
    .serve()
    .await
```

Requires the `ratatui` feature.

### SFTP

Full SFTP server support. Implement the `Filesystem` trait for custom backends,
or use the included `LocalFilesystem` to serve a local directory.

```rust
use shenron::sftp::{Sftp, LocalFilesystem};

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(Sftp::new(LocalFilesystem::new("/srv/files")))
    .app(my_app)
    .serve()
    .await
```

Now clients can `sftp -P 2222 localhost` to browse `/srv/files`, while regular
SSH connections go to your app.

Requires the `sftp` feature.

### Logging

Basic connection logging using `tracing`. Logs session start with remote address,
user, and session type. Logs session end with duration and exit code.

```rust
use shenron::middleware::logging;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(logging)
    .app(my_app)
    .serve()
    .await
```

### Active Terminal

Reject connections without an active PTY. Useful when your app requires a terminal,
like most TUI applications.

```rust
use shenron::middleware::activeterm;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(activeterm)
    .app(my_tui_app)
    .serve()
    .await
```

Clients running `ssh host command` will get rejected. Interactive `ssh host` works fine.

### Access Control

Restrict which commands can be executed via `ssh host command`.

```rust
use shenron::middleware::AccessControl;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(AccessControl::new(["ls", "cat", "echo"]))
    .app(my_app)
    .serve()
    .await
```

Commands not in the allowlist get rejected with exit code 1.

### Rate Limiting

Per-IP rate limiting to prevent abuse.

```rust
use shenron::middleware::RateLimiter;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(RateLimiter::per_minute(10))
    .app(my_app)
    .serve()
    .await
```

Requires the `rate-limiting` feature.

### Elapsed

Print how long the session lasted when it ends.

```rust
use shenron::middleware::elapsed;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(elapsed)
    .app(my_app)
    .serve()
    .await
```

### Comment

Print a message when the session ends.

```rust
use shenron::middleware::Comment;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(Comment("Thanks for stopping by!\r\n".into()))
    .app(my_app)
    .serve()
    .await
```

### Custom Middleware

Writing your own middleware is easy peasy. A middleware is just an async function
that takes a `Session` and a `Next`, and returns a `Result<Session>`.

```rust
use shenron::{Session, Next, Result};

async fn my_middleware(session: Session, next: Next) -> Result {
    // do something before the inner handler runs
    let mut session = next.run(session).await?;
    // do something after the inner handler runs
    Ok(session)
}
```

For middleware with configuration, implement the `Middleware` trait on a struct:

```rust
use shenron::{Middleware, Session, Next, Result};

#[derive(Clone)]
struct Greeter {
    message: String,
}

impl Middleware for Greeter {
    async fn handle(&self, mut session: Session, next: Next) -> Result {
        session.write_str(&self.message).await?;
        next.run(session).await
    }
}
```

Middleware can also short-circuit the chain. This is useful for things like
authentication or rejecting certain session types:

```rust
async fn require_user(session: Session, next: Next) -> Result {
    if session.user() == "admin" {
        next.run(session).await
    } else {
        let mut session = session;
        session.write_stderr_str("Access denied\n").await?;
        Ok(session.exit(1))
    }
}
```

## Pro tips

### Local Development

When building various Shenron apps locally you can add the following to
your `~/.ssh/config` to avoid having to clear out `localhost` entries in your
`~/.ssh/known_hosts` file:

```
Host localhost
    UserKnownHostsFile /dev/null
```

### Running with SystemD

If you want to run a Shenron app with `systemd`, you can create a unit like so:

`/etc/systemd/system/myapp.service`:

```service
[Unit]
Description=My App
After=network.target

[Service]
Type=simple
User=myapp
Group=myapp
WorkingDirectory=/home/myapp/
ExecStart=/usr/bin/myapp
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Once you've done that, you can start it with:

```bash
# need to run this every time you change the unit file
sudo systemctl daemon-reload

sudo systemctl start myapp
```

If you use a new user for each app (which you should), you'll need to create them:

```bash
useradd --system --user-group --create-home myapp
```

## Contributing

See [contributing][contribute].

## Acknowledgements

Shenron is built on the shoulders of giants:

- [russh][russh] — The SSH implementation powering Shenron
- [wish][wish]— The inspiration for this library. If you're building SSH apps in Go, use Wish, it's amazing.
- [ratatui][ratatui] — For making terminal UIs in Rust a joy
- [russh-sftp](https://github.com/AspectUnk/russh-sftp) — SFTP subsystem support

## License

Shenron is licensed under the [MIT License](LICENSE)
