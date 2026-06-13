# Shenron

[wish]: https://github.com/charmbracelet/wish
[ratatui]: https://github.com/ratatui/ratatui
[russh]: https://github.com/Eugeny/russh
[contribute]: https://github.com/MSILycanthropy/Shenron/contribute

🐉 Elegant SSH apps in Rust! 🐉

SSH is a great platform for building applications that are accessible remotely.

- No need for HTTPs certificates like the web
- easy user identification with keys
- access from _any_ terminal

Many protocols like SFTP and Git work over SSH, and you can even render TUIs over SSH
to provide a UI for users.

Shenron is an SSH server with sensible defaults and a collection of middleware to make
building SSH applications painless. Shenron is inspired heavily by [charmbracelet/wish][wish].

Shenron is built on [Eugeny/russh][russh], so OpenSSH is not needed at all. Shenron never spawns a shell or executes commands on its own, your handler decides exactly what every session can do, so there's no `openssh-server` behavior to accidentally expose.

## What is an SSH app?

Typically when connecting via SSH, the server is running something like `openssh-server` to get a shell.

But SSH is much more than that. It's a full cryptographic network protocol like HTTP![^1]

[^1]: https://en.wikipedia.org/wiki/Secure_Shell

That means we can write custom SSH servers that do whatever we want, without using `openssh-server` at all. We can do much much more than just provide a shell, like serve
a TUI to sell coffee.

Shenron is a library that makes writing this type of app in Rust just a wish away.

## Creating an App

Your app is just an async function that borrows the `Session`:

```rust
async fn my_app(session: &mut Session) -> shenron::Result {
    session.write_str("Hello from Shenron!\r\n").await?;
    Ok(())
}
```

The return value becomes the session's exit status, in the spirit of
`std::process::Termination`: `()` exits 0, a `u32` exits with that code, and
an `Err` is logged and exits 1.

```rust
async fn my_app(session: &mut Session) -> shenron::Result<u32> {
    let Some(argv) = session.command() else {
        session.write_stderr_str("usage: ssh host <command>\n").await?;
        return Ok(2);
    };

    // ...

    Ok(0)
}
```

Then, all you need to do is create a server for your app, Shenron ships an always authenticating default SSH server with automatic key generation.

```rust
Server::new()
    .bind("0.0.0.0:2222")
    .app(my_app)
    .serve()
    .await
```

## Authentication

By default Shenron accepts every connection — handy for public apps and local
development. To decide who gets in, add a password and/or public-key handler.
Each returns whether to accept the connection. Once either is configured, only
those methods are advertised to clients and the `none` probe is rejected.

```rust
Server::new()
    .bind("0.0.0.0:2222")
    .password_auth(|user, password| async move {
        user == "admin" && password == "swordfish"
    })
    .app(my_app)
    .serve()
    .await
```

Public-key auth receives the client's key instead of a password:

```rust
use russh::keys::HashAlg;

Server::new()
    .bind("0.0.0.0:2222")
    .pubkey_auth(|user, key| async move {
        key.fingerprint(HashAlg::Sha256).to_string() == "SHA256:abc123..."
    })
    .app(my_app)
    .serve()
    .await
```

For an allowlist, point at an OpenSSH `authorized_keys` file instead of writing
the comparison yourself; for SSH certificates, trust a CA and let it vouch for
users — the helper checks the signature, validity window, principals, and that
it's a user (not host) cert:

```rust
Server::new()
    .pubkey_auth(shenron::auth::authorized_keys(".ssh/authorized_keys")?)
    .cert_auth(shenron::auth::trusted_ca_keys("/etc/ssh/user_ca.pub")?)
    .app(my_app)
```

A handler can return a plain `bool`, or an `Auth` outcome that also attaches
typed data to the session — handy for passing the looked-up account straight to
your app:

```rust
use shenron::Auth;

struct Account { id: u32 }

Server::new()
    .password_auth(|user, password| async move {
        match lookup(&user, &password).await {
            Some(account) => Auth::accept().with(account),
            None => Auth::reject(),
        }
    })
    .app(my_app)
```

Your app reads it back with `session.get::<Account>()` (see
[Working with Sessions](#working-with-sessions)).

### Keyboard-interactive

The methods above answer in one shot. Keyboard-interactive instead runs a
conversation: the handler sends rounds of prompts and reads the client's
answers, which is how you implement OTP codes, MFA, or any challenge sequence.

The handler is called once per connection with a `Challenger`. Each
`challenge` call sends prompts (use `Prompt::hidden` for secrets, `Prompt::echo`
for visible input) and returns the answers in order. Run as many rounds as you
need, then return an `Auth` verdict — `bool`, or `Auth::accept().with(..)` to
attach session data like the other methods:

```rust
use shenron::auth::Prompt;

Server::new()
    .keyboard_interactive_auth(|user, mut ch| async move {
        let code = ch.challenge("", "Two-factor", [Prompt::hidden("OTP code: ")]).await?;

        Ok(Auth::from(verify_otp(&user, &code[0]).await))
    })
    .app(my_app)
```

`challenge` errors only if the client disconnects mid-conversation; propagate it
with `?` and the connection is already gone.

## Host keys

A host key is the server's stable cryptographic identity — it's what lets
clients detect they're still talking to the same server across restarts (the
`known_hosts` check).

When no host key is configured, Shenron generates an Ed25519 key, writes it to
`id_ed25519` (and `id_ed25519.pub`) in the working directory, and reuses it on
the next start. To pick the location yourself, use `host_key_path`, which loads
the key if it exists and generates one if it doesn't:

```rust
Server::new()
    .host_key_path("host_key")?
    .app(my_app)
```

To choose the algorithm of a generated key, or encrypt it with a passphrase,
pass `HostKeyOptions`:

```rust
use shenron::{Algorithm, EcdsaCurve, HostKeyOptions};

Server::new()
    .host_key_path_with(
        "host_key",
        HostKeyOptions::new(Algorithm::Ecdsa { curve: EcdsaCurve::NistP384 })
            .passphrase("correct horse battery staple"),
    )?
    .app(my_app)
```

The passphrase encrypts the key on disk; the running server uses it unencrypted.
Supported algorithms are Ed25519, ECDSA (P-256/P-384/P-521), and RSA (4096-bit).
You can also load an existing key in other ways:

```rust
// Passphrase-encrypted key from a file
.host_key_file_with("host_key", "correct horse battery staple")?

// Raw PEM bytes (e.g. embedded, or pulled from a secret store)
.host_key_pem(include_bytes!("host_key"))?
.host_key_pem_with(pem_bytes, "passphrase")?
```

Add more than one host key and the server offers all of them — the client picks
which to use following the standard SSH host-key preference order. You don't
pick the negotiated algorithm; you pick which keys are available.

## Working with Sessions

Your app and middleware receive a `Session`. Beyond I/O it exposes who connected
and a typed store for carrying data along the chain.

```rust
async fn my_app(session: &mut Session) -> shenron::Result {
    let _user = session.user();
    let _addr = session.remote_addr();

    // The public key the client authenticated with, if any
    if let Some(_key) = session.public_key() {
        // ...
    }

    session.write_str("Hello!\r\n").await?;
    Ok(())
}
```

**Context store.** Each session carries a typed key-value store. Auth handlers
stash data with `Auth::with`; middleware and your app read and write it with
`get` / `get_mut` / `insert`, or take values out with `remove`:

```rust
struct Account { id: u32 }

async fn my_app(session: &mut Session) -> shenron::Result {
    if let Some(account) = session.get::<Account>() {
        let msg = format!("Welcome back, #{}\r\n", account.id);
        session.write_str(&msg).await?;
    }
    Ok(())
}
```

Some commonly used session methods:

- `user()` / `remote_addr()` / `public_key()` — connection identity
- `kind()`, `command()`, `pty()`, `term()`, `env()` — what the client requested.
  `kind()` borrows a `SessionKind`; `command()` is the POSIX-parsed argv of an
  exec request (`raw_command()` gives the unparsed string)
- `next().await` — the event stream: `Input`, `Resize`, `Signal`, `Eof`
- `write_str` / `write` / `write_stderr_str` — output
- `get::<T>()` / `get_mut::<T>()` / `remove::<T>()` / `insert(value)` — the context store
- the handler's return value reports the exit code; `abort(code)` ends the
  session early without waiting for the handler to return

## Server configuration

Show a banner before authentication:

```rust
Server::new()
    .banner("Authorized users only.\r\n")  // or .banner_file("banner.txt")?
    .app(my_app)
```

Throttle failed auth attempts, cap idle connections, and detect dead peers
with keepalives:

```rust
use std::time::Duration;

Server::new()
    .auth_rejection_delay(Duration::from_secs(2))            // stall failed auth attempts
    .auth_rejection_delay_initial(Duration::from_millis(50)) // but fail the `none` probe fast
    .inactivity_timeout(Duration::from_secs(600))            // drop idle sessions
    .keepalive_interval(Duration::from_secs(15))             // ping the client
    .keepalive_max(3)                                        // give up after N missed pings
    .app(my_app)
```

Stop accepting new connections when a future completes:

```rust
Server::new()
    .shutdown_signal(async {
        tokio::signal::ctrl_c().await.ok();
    })
    .app(my_app)
```

## Terminal UIs

With the `ratatui` feature, your app can drive the session as a
[ratatui][ratatui] TUI. `session.tui()` returns a handle that renders to the
client's terminal, parses its input into key and paste events, and follows
window resizes automatically. The type parameter is your own message type,
letting background tasks push into the event loop through `sender()`:

```rust
use ratatui::crossterm::event::KeyCode;
use shenron::{Result, Server, Session, tui};

enum Msg {
    Tick,
}

async fn counter(session: &mut Session) -> Result {
    let mut tui = session.tui::<Msg>()?.alt_screen();

    // Server-push: wake the loop from a background task.
    let tx = tui.sender();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if tx.send(Msg::Tick).is_err() {
                break;
            }
        }
    });

    let mut ticks = 0;
    loop {
        tui.draw(|frame| {
            // draw your UI with ratatui
        })
        .await?;

        match tui.next().await {
            Some(tui::Event::Key(key)) if key.code == KeyCode::Char('q') => break,
            Some(tui::Event::App(Msg::Tick)) => ticks += 1,
            Some(tui::Event::Eof) | None => break,
            _ => {}
        }
    }

    tui.close().await
}
```

Events are either client input (`Key`, `Paste`, `Resize`, `Eof`) or your own
messages (`App`). `session.tui()` errors when the client didn't request a PTY,
so pair it with the [`active_term`](#active-terminal) middleware to reject
those sessions up front. See the full [TUI example](examples/tui.rs).

## Examples

There are examples for a standalone [Ratatui app](examples/tui.rs) and others in the [examples](examples) folder.

## Middleware

Shenron middleware works like middleware in most HTTP frameworks. Each middleware borrows the session, letting you act before and after lending it down the chain. `next.run` resolves the rest of the chain to an `Exit` — failures arrive as `Exit::Error` rather than `Err`, so you inspect rather than `?`:

```rust
async fn my_middleware(session: &mut Session, next: Next<'_>) -> Exit {
    // do stuff before
    let exit = next.run(session).await;
    // do stuff after
    exit
}
```

Middleware is composed outside-in — the first middleware you add is the outermost layer,
meaning it sees the session first and the result last.

```rust
Server::new()
    .bind("0.0.0.0:2222")
    .host_key_path("host_key")?
    .with(logging)        // 1st: sees session first, result last
    .with(active_term)    // 2nd: runs inside logging
    .app(my_app)          // innermost: your application
    .serve()
    .await
```

## Built-In Middleware

Shenron ships with a collection of middleware to handle common tasks.

### SFTP

Full SFTP server support. Implement the async `Filesystem` trait for custom
backends (network-backed stores can be natively async), or serve a local
directory with `Sftp::local`:

```rust
use shenron::sftp::Sftp;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(Sftp::local("/srv/files"))
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

### Recover

Contain a panicking handler or middleware instead of letting it drop the session
abruptly. The panic is logged via `tracing` (with the user and remote address)
and converted into an `Exit::Error` (exit 1), so the connection closes cleanly
and the server and other sessions keep running.

```rust
use shenron::middleware::{logging, recover};

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(logging)   // outside recover: still logs the session ending
    .with(recover)   // catches panics in everything below it
    .app(my_app)
    .serve()
    .await
```

Placement matters: `recover` only catches panics from the middleware and app
*inside* it. Put it just inside your observability middleware (`logging`,
`elapsed`, `Comment`) so a panic becomes an error those outer layers still see —
their "after" logic runs and you keep the disconnect log. A panic in middleware
placed *outside* `recover` is not caught.

To also forward panics somewhere (metrics, error reporting), use `recover_with`:

```rust
use shenron::middleware::recover_with;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(recover_with(|report| {
        metrics::increment(&report.user, report.message);
    }))
    .app(my_app)
    .serve()
    .await
```

`recover` relies on unwinding panics, which is the default. If your build profile
sets `panic = "abort"`, the process aborts before anything can be recovered.

### Active Terminal

Reject connections without an active PTY. Useful when your app requires a terminal,
like most TUI applications.

```rust
use shenron::middleware::active_term;

Server::new()
    .bind("0.0.0.0:2222")
    .host_key_file("host_key")?
    .with(active_term)
    .app(my_tui_app)
    .serve()
    .await
```

Clients running `ssh host command` will get rejected. Interactive `ssh host` works fine.

### Access Control

Restrict which programs can be executed via `ssh host command`. The check
compares the *program* — `argv[0]` of the POSIX-parsed command — exactly
against the allowlist, so allowing `git` permits `git push` and `git pull`
alike.

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

Commands not in the allowlist get rejected with exit code 1. Sessions without
an exec command (shells, subsystems) pass through untouched. Note this is only
a security boundary if your app executes the parsed argv directly — never hand
`raw_command()` to a shell.

### Rate Limiting

Per-IP rate limiting for established sessions. Because it runs as middleware,
it throttles authenticated session rates rather than raw connection or
failed-auth floods — pair it with a firewall if you need to protect the
handshake itself.

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

Writing your own middleware is easy peasy. A middleware is just an async
function that takes a `&mut Session` and a `Next`, and returns anything
`IntoExit` — `Exit` itself, or `Result<Exit>` when you want `?` for your own
I/O:

```rust
use shenron::{Exit, Next, Result, Session};

async fn my_middleware(session: &mut Session, next: Next<'_>) -> Result<Exit> {
    // do something before the inner handler runs
    let exit = next.run(session).await;
    // do something after the inner handler runs
    Ok(exit)
}
```

For middleware with configuration, implement the `Middleware` trait on a struct:

```rust
use shenron::{Exit, Middleware, Next, Result, Session};

struct Greeter {
    message: String,
}

impl Middleware for Greeter {
    type Output = Result<Exit>;

    async fn handle(&self, session: &mut Session, next: Next<'_>) -> Result<Exit> {
        session.write_str(&self.message).await?;
        Ok(next.run(session).await)
    }
}
```

Middleware can also short-circuit the chain by returning without calling
`next.run`. This is useful for things like authentication or rejecting certain
session types:

```rust
async fn require_user(session: &mut Session, next: Next<'_>) -> Result<Exit> {
    if session.user() == "admin" {
        Ok(next.run(session).await)
    } else {
        session.write_stderr_str("Access denied\n").await?;
        Ok(Exit::Code(1))
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
