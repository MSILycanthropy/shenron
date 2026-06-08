# Handoff: shenron TUI rework — borrow-model middleware + own-the-loop + server-push

**Repo:** `/Users/ethankircher/testspaces/shenron` (Rust, edition 2024; russh 0.61.2 + ratatui 0.30.1, crossterm via ratatui). Branch `main`.

**Status:** Design fully resolved **and the hard typesystem questions empirically validated by compiling prototypes** (see §4). **No production code written yet** — every `src/` change below is still TODO. The earlier `src/tui/*` / `src/middleware/*` / `src/handler.rs` are still in their original (pre-rework) state. This doc is the spec; implement it.

This supersedes the prior handoff (`/var/folders/.../shenron-tui-rework-handoff.md`). That doc designed an **own-the-loop API on the existing move-based middleware contract**. During implementation we changed course: the user pushed for middleware to borrow the session (`&mut Session`) rather than thread it by value, which is cleaner and removes the awkward `into_session().exit(0)` ceremony. That requires **nightly Rust**. The user explicitly approved nightly.

---

## 1. TL;DR of what changed vs. the prior handoff

The prior handoff still holds for these points (unchanged):
1. **The minimal-bandwidth diffing already works — do not touch it.** The original "Shenron sends only changed cells" brief was a false premise (Terminal built once, `clear()` once at startup, all rendering via `terminal.draw`). `SessionWriter` is correct.
2. **Real latent bug to fix (autoresize):** `Terminal::new()` defaults to `Viewport::Fullscreen`; every `draw()` calls `autoresize()` → `crossterm::terminal::size()` which reads the **server's local** terminal, not the client PTY. Fix by using **`Viewport::Fixed`** (autoresize skips Fixed) and driving size from PTY/WindowChange events via `Terminal::resize`. Fold into the Tui rewrite. (Verified call chain: `ratatui-core-0.1.1/src/terminal/render.rs:196` `try_draw`→`autoresize`; `.../resize.rs:66` queries `self.size()` only for Fullscreen/Inline; `ratatui-crossterm-0.1.1/src/lib.rs:337-338` `terminal::size()` is local.)
3. **Own-the-loop API (Option B):** the app is a plain `async fn` you drive yourself, not an IoC trait. **Delete the `App` trait + `Ratatui<A>` runner.**
4. **Headline feature: server-pushed redraws** via an unbounded mpsc whose `sender()` can be handed to a user-owned registry. shenron ships **no** registry.
5. **Unify `Handler` + `Middleware` into one trait** (β′): delete `Handler`, keep one `Middleware`, app = innermost terminal middleware via a `terminal(f)` adapter; `.app()` survives as sugar.
6. **Scope is a leaf rework.** Foundation (`Server`, russh glue, auth, the onion) stays. Forwarding (port/agent/X11) stays out.

**What this handoff changes from the prior one:**
- **Middleware contract flips from MOVE to BORROW.** Was `Fn(Session, Next) -> Result<Session>`. Now `for<'a> Fn(&'a mut Session, Next<'a>) -> Result<()>`. The server owns the `Session`; every layer borrows `&mut`. No more returning the session.
- **Leaf ergonomics:** app is `async fn(session: &mut Session) -> Result<()>`. Ends with `session.exit(0)` (now returns `Result<()>`). `Events`/`Tui` **borrow** the session (`Events<'a, M>`, `Tui<'a, M>`); no `into_session()`. `Tui::close().await?` is the one explicit async teardown (terminal restore).
- **Requires nightly** (`#![feature(async_fn_traits, unboxed_closures)]` + a `rust-toolchain.toml` pinning `nightly`). Validated on `rustc 1.93.0-nightly`.
- **`recover` builtin** can no longer use its `tokio::spawn(next.run(session))` trick (can't move a `&mut` into a `'static` spawn). Replace with a dependency-free `catch_unwind` poll loop (§6, `recover`).

---

## 2. Why the borrow model (the conversation that led here)

The user's objection to the move model was the leaf ceremony: ending an app with `events.into_session().exit(0)` felt wrong. They asked: *"can we change the Middleware contract... each middleware should easily be able to own the session mutably without returning it?"*

That instinct is correct. In a **borrow** model the server owns the `Session` at the top and passes `&mut Session` down the onion. A middleware does its before-work, calls `next.run(session)` (re-borrow), then its after-work — **still holding its `&mut`**. After-app middleware (e.g. `Comment` writing a goodbye) works fine; I was briefly wrong that returning `()` would kill it — that's only true in the *move* model where `()` drops the session. In the borrow model the session never moves into the chain, so "after" work is always available.

The leaf borrows the session into a `Tui<'a>`/`Events<'a>` handle for the loop, drops the handle, and the `&mut Session` is usable again to set the exit code. Clean.

The only cost is a Rust typesystem wrinkle (closures returning a future that borrows their `&mut` arg), resolved on nightly — see §4.

---

## 3. The chosen API shape (what the user will write)

```rust
use shenron::{Result, Session, Server, Next};
use shenron::tui;                     // tui::Event, Tui live here (cfg "ratatui")

enum Msg { Tick }                     // app message type for server-push

// A terminal app: borrows &mut Session, returns Result<()>.
async fn counter(session: &mut Session) -> Result<()> {
    let mut tui = session.tui::<Msg>()?;   // borrows the session; errors if no PTY

    // server-push: ticker wakes the loop once a second
    let tx = tui.sender();                 // UnboundedSender<Msg>, 'static — movable into a task
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(std::time::Duration::from_secs(1));
        loop { iv.tick().await; if tx.send(Msg::Tick).is_err() { break; } }
    });

    let mut count = 0i32;
    loop {
        tui.draw(|f| draw_ui(f, count)).await?;     // async: sync ratatui draw + async flush
        match tui.next().await {                     // merges keys + Msg in one match
            Some(tui::Event::Key(k))  => { /* mutate count; break on q/ctrl-c */ }
            Some(tui::Event::App(Msg::Tick)) => { /* redraw on next loop */ }
            Some(tui::Event::Resize(_)) => {}        // Tui already resized its Terminal
            Some(tui::Event::Eof) | None => break,
        }
    }

    tui.close().await?;     // restore terminal (REQUIRED if alt_screen); releases the borrow
    session.exit(0)         // Result<()> — clean final expression
}

// A middleware: borrows &mut Session, can act before AND after.
async fn log(session: &mut Session, next: Next<'_>) -> Result<()> {
    tracing::info!("{} connected", session.user());
    next.run(session).await?;          // re-borrow down the chain
    tracing::info!("disconnected");    // after-app work — session still &mut here
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    Server::new().bind("0.0.0.0:2222").with(log).app(counter).serve().await
}
```

Non-TUI own-the-loop apps use `session.events::<M>()` → `Events<'a, M>` with `events::Event<M>` (adds a `Signal` variant; no terminal, no `close()` needed — just drop).

---

## 4. The typesystem validation (DO NOT re-derive — this was the hard part)

The blocker: a closure/`async fn` that takes `&mut Session` and returns a future **borrowing** it can't be bound by the naive `F: Fn(&mut Session) -> Fut` (the future's lifetime depends on the input borrow; `Fut` is one type). Four prototypes (`/tmp/borrow_proto*.rs`) nailed down what works:

- **proto1 (stable, `AsyncFn` in where-clause):** fails — `CallRefFuture` is unstable (`async_fn_traits`) **and** "implementation of `Send` is not general enough" (the two-lifetime HRTB hole).
- **proto2 (stable, struct-only borrow model):** **compiles.** Struct `impl Middleware` + struct apps work on stable. But bare `async fn` closures do not.
- **proto3 (nightly, `async fn handle` wrapper blanket):** feature gate clears, but **"Send is not general enough" persists** — caused by the extra future from the `async fn` *wrapper*.
- **proto4 (nightly, direct-return blanket):** Send error gone; now E0700 "hidden type captures lifetime that does not appear in bounds" — the RPIT must name the borrow lifetime.
- **proto5 / proto6 (nightly, explicit-lifetime trait method + direct-return blanket):** **compiles clean (exit 0).** proto6 combines: bare `async fn` middleware, a struct middleware writing AFTER the app, a bare `async fn` app via `terminal()`, and a leaf that borrows the session into a `Tui<'a>` then keeps using the session. This is the recipe.

**The exact recipe that compiles (from proto6):**

```rust
#![feature(async_fn_traits)]
#![feature(unboxed_closures)]      // needed for the AsyncFnMut<(..)> angle-bracket form in the Send bound

use std::ops::AsyncFnMut;          // for naming CallRefFuture

pub trait Middleware: Send + Sync + 'static {       // NOTE: no Clone needed (borrow model doesn't clone)
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>)
        -> impl Future<Output = Result<()>> + Send + 'a;     // explicit 'a + `+ 'a` on the future
}

// Closure blanket — DIRECT RETURN (no `async fn` wrapper, or Send breaks):
impl<F> Middleware for F
where
    F: AsyncFn(&mut Session, Next<'_>) -> Result<()> + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session, Next<'a>)>>::CallRefFuture<'a>: Send,
{
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>)
        -> impl Future<Output = Result<()>> + Send + 'a
    {
        self(session, next)        // returns CallRefFuture<'a>; Send holds per-call lifetime
    }
}
```

Key lessons baked in: (1) explicit `<'a>` on the method with `+ 'a` on the RPIT; (2) **return the call future directly**, never wrap in `async {}`; (3) the `for<'a> <F as AsyncFnMut<...>>::CallRefFuture<'a>: Send` bound is mandatory and needs `unboxed_closures` for the tuple syntax.

The full validated prototypes are at `/tmp/borrow_proto2.rs` (stable struct-only) and `/tmp/borrow_proto6.rs` (nightly, full). If `/tmp` is gone, the recipe above + §6 reconstruct them. **Compile commands used:** `rustc +nightly --edition 2024 --crate-type lib borrow_proto6.rs` → exit 0.

---

## 5. Module layout (target)

```
src/
  lib.rs                 # add #![feature(async_fn_traits, unboxed_closures)]; re-exports; drop `mod handler`
  handler.rs             # DELETE
  error.rs               # unchanged (Error::Panic stays)
  events/                # NEW — feature-INDEPENDENT (NOT behind ratatui)
    mod.rs               #   pub use core::Events; pub use event::Event; pub use interceptor::{Interceptor, Interceptors};
    core.rs              #   Events<'a, M>
    event.rs             #   events::Event<M> { Input, Resize, Signal, App(M), Eof } + From<crate::Event>
    interceptor.rs       #   Interceptor<E> trait + Interceptors<E> combinator
  tui/                   # cfg(feature = "ratatui")
    mod.rs               #   pub use core::Tui; pub use event::Event; pub use key::parse_key_event;
    core.rs              #   Tui<'a, M> (Viewport::Fixed; async draw/next/close; sender; write*)
    event.rs             #   tui::Event<M> { Key, Resize, App(M), Eof }
    key.rs               #   parse_key_event (MOVED verbatim from old app.rs, incl. tests) + parse_utf8_char
    writer.rs            #   SessionWriter (KEEP as-is)
  middleware/
    mod.rs               #   exports (add terminal, Terminal)
    core.rs              #   Middleware trait + AsyncFn blanket + terminal()/Terminal<F>
    next.rs              #   Next<'a> { run(self, &mut Session) -> Result<()> }
    erased.rs            #   ErasedMiddleware + ErasedHandler (lifetime'd; NO Handler blanket)
    chain.rs             #   build_chain(Vec<Arc<dyn ErasedMiddleware>>) -> Arc<dyn ErasedHandler>; Base; MiddlewareHandler
    builtins/            #   all rewritten to &mut Session / Result<()>
  server/
    core.rs              #   .with()/.app()/serve(); drop `app` field + Handler import
    russh.rs             #   run_handler owns Session, passes &mut, do_exit after
  session/
    core.rs              #   exit/abort -> Result<()> on &mut self; add events()/tui()
    event.rs, kind.rs, pty.rs, mod.rs  # unchanged (crate::Event stays for raw Session::next)
```

**Why `events/` is not under `tui/`:** default features = `[]` (ratatui OFF). `Events`/`events::Event`/`Interceptor` must compile without ratatui, so they live in their own always-compiled module. `Tui` (cfg `ratatui`) is built on top of `Events`.

**Two `Event` types is intentional** (namespaced): `crate::Event` (existing, non-generic, from `Session::next`), `shenron::events::Event<M>` (adds `App(M)`), `shenron::tui::Event<M>` (Key instead of Input, no Signal). `Tui::next()` maps `events::Event::Input(bytes)` → `tui::Event::Key(parse_key_event(bytes))`, skips unparseable input + `Signal`, passes the rest through.

---

## 6. File-by-file implementation spec

### `rust-toolchain.toml` (NEW, repo root)
```toml
[toolchain]
channel = "nightly"
components = ["rustfmt", "clippy"]
```

### `src/lib.rs`
- Add at very top: `#![feature(async_fn_traits, unboxed_closures)]`.
- Remove `mod handler;` and `pub use handler::Handler;`.
- Add `pub mod events;` (before `tui`).
- Re-exports: `pub use events::Events;`, `pub use middleware::{Middleware, Next, terminal};` (add `terminal`). Keep `pub use session::{Event, PtySize, Session, SessionKind, Signal};`. Keep `BoxFuture` if still used — but note it becomes lifetime'd (see erased.rs); likely replace the crate alias with a local `BoxFuture<'a, T>` in `middleware`.
- `BoxFuture` change: old `pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output=T> + Send>>`. New needs a lifetime: `pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;`. Update the one place it's used (erased.rs). (sftp uses its own futures; check it doesn't depend on the old alias.)

### `src/middleware/core.rs` (the heart — see §4 recipe)
```rust
use std::ops::AsyncFnMut;
use crate::{Next, Result, Session};

pub trait Middleware: Send + Sync + 'static {
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>)
        -> impl Future<Output = Result<()>> + Send + 'a;
}

impl<F> Middleware for F
where
    F: AsyncFn(&mut Session, Next<'_>) -> Result<()> + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session, Next<'a>)>>::CallRefFuture<'a>: Send,
{
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>)
        -> impl Future<Output = Result<()>> + Send + 'a
    { self(session, next) }
}

/// Adapt a terminal app `Fn(&mut Session) -> Result<()>` as middleware that ignores `next`.
pub struct Terminal<F>(F);
pub fn terminal<F>(f: F) -> Terminal<F>
where
    F: AsyncFn(&mut Session) -> Result<()> + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
{ Terminal(f) }

impl<F> Middleware for Terminal<F>
where
    F: AsyncFn(&mut Session) -> Result<()> + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
{
    fn handle<'a>(&'a self, session: &'a mut Session, _next: Next<'a>)
        -> impl Future<Output = Result<()>> + Send + 'a
    { (self.0)(session) }
}
```
Note: **no `Clone`** on `Middleware` (the borrow-model erased layer doesn't clone). Keep the doc-comment example but update it to the borrow signature.

### `src/middleware/next.rs`
```rust
use crate::{Result, Session, middleware::ErasedHandler};

pub struct Next<'a> { inner: &'a dyn ErasedHandler }
impl<'a> Next<'a> {
    pub(crate) fn new(inner: &'a dyn ErasedHandler) -> Self { Self { inner } }
    pub async fn run(self, session: &mut Session) -> Result<()> { self.inner.call(session).await }
}
```

### `src/middleware/erased.rs`
```rust
use std::pin::Pin;
use crate::{Middleware, Next, Result, Session};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait ErasedMiddleware: Send + Sync {
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>) -> BoxFuture<'a, Result<()>>;
}
impl<M: Middleware> ErasedMiddleware for M {
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>) -> BoxFuture<'a, Result<()>> {
        Box::pin(Middleware::handle(self, session, next))
    }
}

pub(crate) trait ErasedHandler: Send + Sync {
    fn call<'a>(&'a self, session: &'a mut Session) -> BoxFuture<'a, Result<()>>;
}
// NOTE: NO `impl<H: Handler> ErasedHandler` blanket anymore. Handler is deleted.
// ErasedHandler is implemented only by Base and MiddlewareHandler (in chain.rs).
```

### `src/middleware/chain.rs`
```rust
use std::sync::Arc;
use std::pin::Pin;
use crate::{Next, Result, Session, middleware::{ErasedHandler, ErasedMiddleware}};
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) fn build_chain(middleware: Vec<Arc<dyn ErasedMiddleware>>) -> Arc<dyn ErasedHandler> {
    let mut chain: Arc<dyn ErasedHandler> = Arc::new(Base);
    for mw in middleware.into_iter().rev() {
        chain = Arc::new(MiddlewareHandler { middleware: mw, next: chain });
    }
    chain
}

struct Base;
impl ErasedHandler for Base {
    fn call<'a>(&'a self, _session: &'a mut Session) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
struct MiddlewareHandler { middleware: Arc<dyn ErasedMiddleware>, next: Arc<dyn ErasedHandler> }
impl ErasedHandler for MiddlewareHandler {
    fn call<'a>(&'a self, session: &'a mut Session) -> BoxFuture<'a, Result<()>> {
        let next = Next::new(self.next.as_ref());
        self.middleware.handle(session, next)
    }
}
```

### `src/middleware/mod.rs`
Add `terminal` + `Terminal` to the `core` re-export (already `pub use core::*`). Ensure `Next` still exported. Keep `pub(crate) use erased::*; pub(crate) use chain::*;`.

### `src/session/core.rs`
- `exit`: change from `pub const fn exit(mut self, code) -> Result<Self>` to:
  ```rust
  #[allow(clippy::missing_errors_doc)]
  pub fn exit(&mut self, code: u32) -> Result<()> { self.exit_code = Some(code); Ok(()) }
  ```
- `abort`: `pub async fn abort(&mut self, code: u32) -> Result<()> { self.exit_code = Some(code); self.do_exit().await }`.
- Add (feature-independent):
  ```rust
  pub fn events<M>(&mut self) -> crate::events::Events<'_, M> { crate::events::Events::new(self) }
  ```
- Add (cfg ratatui):
  ```rust
  #[cfg(feature = "ratatui")]
  pub fn tui<M>(&mut self) -> Result<crate::tui::Tui<'_, M>> { crate::tui::Tui::new(self) }
  ```
- `do_exit`, `take_channel`, `next`, `write*`, getters: unchanged.
- Methods that are generic over `M` cannot default `M` (Rust forbids defaulted method type-params). Users write `session.tui::<Msg>()`, or annotate the binding `let mut tui: Tui = session.tui()?;` to get the `M = ()` type default. Document this.

### `src/events/event.rs`
```rust
use crate::{PtySize, Signal};
#[derive(Debug)]
pub enum Event<M = ()> { Input(Vec<u8>), Resize(PtySize), Signal(Signal), App(M), Eof }
impl<M> From<crate::Event> for Event<M> {
    fn from(e: crate::Event) -> Self {
        match e {
            crate::Event::Input(d) => Self::Input(d),
            crate::Event::Resize(s) => Self::Resize(s),
            crate::Event::Signal(s) => Self::Signal(s),
            crate::Event::Eof => Self::Eof,
        }
    }
}
```

### `src/events/core.rs`  (Events BORROWS the session)
```rust
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use crate::{Result, Session, events::Event};

pub struct Events<'a, M = ()> {
    session: &'a mut Session,
    sender: UnboundedSender<M>,
    receiver: UnboundedReceiver<M>,
}
impl<'a, M> Events<'a, M> {
    pub(crate) fn new(session: &'a mut Session) -> Self {
        let (sender, receiver) = unbounded_channel();
        Self { session, sender, receiver }
    }
    /// Merge SSH input + app messages. Cancel-safe (Channel::wait = mpsc recv; tokio recv cancel-safe).
    pub async fn next(&mut self) -> Option<Event<M>> {
        tokio::select! {
            event = self.session.next() => event.map(Into::into),
            Some(msg) = self.receiver.recv() => Some(Event::App(msg)),
        }
    }
    pub fn sender(&self) -> UnboundedSender<M> { self.sender.clone() }   // 'static; movable into tasks
    pub async fn write(&self, data: &[u8]) -> Result<()> { self.session.write(data).await }
    pub async fn write_str(&self, s: &str) -> Result<()> { self.session.write_str(s).await }
    // NO into_session(): just drop to release the &mut borrow; caller then uses `session` again.
    // (Provide `pub fn session(&mut self) -> &mut Session` only if a real need shows up.)
}
```
Cancel-safety note (carry-over from prior handoff, **confirmed**): `russh::ChannelReadHalf::wait`/`Channel::wait` is `self.receiver.recv().await` over a tokio mpsc (cancel-safe). So the `select!` needs no reader task. `Session::next` is a skip-protocol loop awaiting `wait()`; dropping it mid-await loses nothing. Verified at `russh-0.61.2/src/channels/mod.rs:156,655`.

### `src/events/interceptor.rs`  (free-standing, applied inline by the user — NO registration)
```rust
pub trait Interceptor<E> { fn apply(&mut self, event: E) -> Option<E>; }   // Some = pass/transform, None = drop
impl<E, F: FnMut(E) -> Option<E>> Interceptor<E> for F {
    fn apply(&mut self, event: E) -> Option<E> { self(event) }
}
#[derive(Default)]
pub struct Interceptors<E>(Vec<Box<dyn Interceptor<E> + Send>>);
impl<E> Interceptors<E> {
    pub fn new() -> Self { Self(Vec::new()) }
    pub fn with(mut self, i: impl Interceptor<E> + Send + 'static) -> Self { self.0.push(Box::new(i)); self }
}
impl<E> Interceptor<E> for Interceptors<E> {
    fn apply(&mut self, mut event: E) -> Option<E> {
        for i in &mut self.0 { event = i.apply(event)?; }
        Some(event)
    }
}
```
Usage in the user's own loop: `let Some(ev) = fx.apply(ev) else { continue };` — identical for `events::Event` and `tui::Event` (generic over `E`).

### `src/tui/event.rs`
```rust
use ratatui::crossterm::event::KeyEvent;
use crate::PtySize;
#[derive(Debug)]
pub enum Event<M = ()> { Key(KeyEvent), Resize(PtySize), App(M), Eof }
```

### `src/tui/key.rs`
Move `parse_key_event` + `parse_utf8_char` + the `#[cfg(test)] mod tests` **verbatim** from the old `src/tui/app.rs` (lines 100-241). Keep `#[must_use] pub fn parse_key_event`.

### `src/tui/core.rs`  (Tui BORROWS via Events)
```rust
use std::io::Write;
use ratatui::{Frame, Terminal as RatatuiTerminal, TerminalOptions, Viewport,
              layout::Rect, prelude::CrosstermBackend};
use crate::{Error, Result, Session,
            events::{Events, Event as RawEvent},
            tui::{event::Event, key::parse_key_event, writer::SessionWriter}};

type Backend = CrosstermBackend<SessionWriter>;

pub struct Tui<'a, M = ()> {
    events: Events<'a, M>,
    terminal: RatatuiTerminal<Backend>,
    alt_screen: bool,
    entered: bool,     // alt-screen entered lazily on first draw
}

impl<'a, M> Tui<'a, M> {
    pub(crate) fn new(session: &'a mut Session) -> Result<Self> {
        let Some(pty_size) = session.pty_size() else {
            return Err(Error::Protocol("tui requires a pty".into()));
        };
        let area: Rect = pty_size.try_into()?;     // PtySize: TryFrom<PtySize> for Rect exists (cfg ratatui)
        let terminal = RatatuiTerminal::with_options(
            CrosstermBackend::new(SessionWriter::new()),
            TerminalOptions { viewport: Viewport::Fixed(area) },   // THE autoresize fix
        )?;
        Ok(Self { events: Events::new(session), terminal, alt_screen: false, entered: false })
    }

    #[must_use] pub fn alt_screen(mut self) -> Self { self.alt_screen = true; self }   // opt-in, OFF by default
    pub fn sender(&self) -> tokio::sync::mpsc::UnboundedSender<M> { self.events.sender() }
    pub async fn write(&self, d: &[u8]) -> Result<()> { self.events.write(d).await }
    pub async fn write_str(&self, s: &str) -> Result<()> { self.events.write_str(s).await }

    /// Sync ratatui draw, then async flush of the SessionWriter to the channel.
    /// Honors the frame cursor automatically (ratatui's try_draw shows/hides per frame.cursor_position) —
    /// DO NOT force-hide like the old code.
    pub async fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> Result<()> {
        if self.alt_screen && !self.entered {
            self.terminal.backend_mut().writer_mut().write_all(b"\x1b[?1049h")?;
            self.entered = true;
        }
        self.terminal.draw(render)?;
        let data = self.terminal.backend_mut().writer_mut().take();
        self.events.write(&data).await
    }

    pub async fn next(&mut self) -> Option<Event<M>> {
        loop {
            match self.events.next().await? {
                RawEvent::Input(bytes) => {
                    if let Some(key) = parse_key_event(&bytes) { return Some(Event::Key(key)); }
                    // unparseable: skip, keep looping
                }
                RawEvent::Resize(size) => {
                    if let Ok(rect) = size.try_into() { let _ = self.terminal.resize(rect); }  // Fixed needs explicit resize
                    return Some(Event::Resize(size));
                }
                RawEvent::Signal(_) => {}      // not surfaced in tui::Event
                RawEvent::App(m) => return Some(Event::App(m)),
                RawEvent::Eof => return Some(Event::Eof),
            }
        }
    }

    /// Restore terminal state (show cursor; leave alt-screen if entered). Best-effort-ish but returns Result
    /// so the caller can `?`. REQUIRED before exit when alt_screen is on, else the client stays in alt-screen.
    pub async fn close(self) -> Result<()> {
        let mut restore: Vec<u8> = b"\x1b[?25h".to_vec();      // show cursor
        if self.alt_screen && self.entered { restore.extend_from_slice(b"\x1b[?1049l"); }
        else { restore.extend_from_slice(b"\r\n"); }           // inline: drop below the UI
        self.events.write(&restore).await
        // self (and the &mut borrow) dropped here → caller's `session` is usable again
    }
}
```
Confirmed ratatui 0.30.1 API: `ratatui::{Terminal, TerminalOptions, Viewport}` (re-exported at `ratatui-0.30.1/src/lib.rs:476`); `Viewport::Fixed(Rect)`; Fixed is NOT autoresized (must call `Terminal::resize`); `try_draw` honors `frame.cursor_position` via `apply_buffer_with_cursor`. `writer_mut()` available (`unstable-backend-writer` feature, already enabled in Cargo.toml).

### `src/tui/mod.rs`
```rust
pub mod core;
mod event;
mod key;
pub(crate) mod writer;
pub use core::Tui;
pub use event::Event;
pub use key::parse_key_event;
```
(Old `pub use app::{App, Ratatui};` is gone — `App`/`Ratatui` deleted with `app.rs`.)

### `src/server/core.rs`
- Drop `Handler` import; drop the `app: Option<Arc<dyn ErasedHandler>>` field. Add `has_app: bool` (preserve the "No app handler specified" error).
- `.with<M: Middleware>(...)` unchanged (push `Arc::new(mw)`). (Drop the `+ Clone` requirement if present — Middleware no longer requires Clone.)
- `.app`:
  ```rust
  pub fn app<F>(mut self, f: F) -> Self
  where F: AsyncFn(&mut Session) -> Result<()> + Send + Sync + 'static,
        for<'a> <F as std::ops::AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
  {
      self.middleware.push(Arc::new(crate::middleware::terminal(f)));   // innermost == last
      self.has_app = true;
      self
  }
  ```
- `serve()`: replace handler extraction with:
  ```rust
  if !self.has_app { return Err(crate::Error::Config("No app handler specified".into())); }
  let handler = middleware::build_chain(std::mem::take(&mut self.middleware));
  ```
  Pass `handler` into `ShenronServer` as before. (App must be added last so it's innermost; document. `.with()` after `.app()` would nest inside the terminal app, which ignores `next` — harmless no-op.)

### `src/server/russh.rs`
`run_handler` now owns the Session and lends `&mut`:
```rust
fn run_handler(handler: Arc<dyn ErasedHandler>, mut session: Session) {
    tokio::spawn(async move {
        match handler.call(&mut session).await {
            Ok(()) => { let _ = session.do_exit().await; }
            Err(e) => tracing::error!("Handler error: {}", e),
        }
    });
}
```
Everything else in russh.rs unchanged (identity already populated at `:148,:209`). `do_exit` only sends `exit_status` if `exit_code` is `Some` — so apps that forget `exit(0)` just close without a status (same as today).

### `src/middleware/builtins/*` — rewrite to borrow model
All change signature `(&mut Session, Next<'_>) -> Result<()>`, and `next.run(session)` now returns `Result<()>`. Specifics:

- **`comment.rs`** (`Comment(String)`): `next.run(session).await?; session.write_str(&self.0).await?; Ok(())`. (After-app write — works.)
- **`elapsed.rs`**: capture `Instant` before, `next.run(session).await?`, log elapsed, `Ok(())`. (Check its current return handling.)
- **`logging.rs`**: before/after logs around `next.run(session).await`.
- **`active_term.rs`, `access_control.rs`, `rate_limit.rs`**: mechanical signature flip; they branch then call `next.run(session)` or short-circuit with `Ok(())`.
- **`recover.rs`** — the one with real surgery. Old approach moves the session into `tokio::spawn(next.run(session))` to catch panics at the task boundary. **Can't move `&mut Session` into a `'static` spawn.** Replace with an in-place `catch_unwind` poll loop (no new deps):
  ```rust
  use std::future::poll_fn;
  use std::panic::AssertUnwindSafe;
  use std::task::Poll;

  pub async fn recover(session: &mut Session, next: Next<'_>) -> Result<()> {
      let user = session.user().to_owned();
      let remote = session.remote_addr();
      let mut fut = Box::pin(next.run(session));     // Box::pin avoids unsafe pin projection
      let outcome = poll_fn(|cx| {
          match std::panic::catch_unwind(AssertUnwindSafe(|| fut.as_mut().poll(cx))) {
              Ok(poll) => poll.map(Ok),               // Poll<Result<()>> -> Poll<std::thread::Result<Result<()>>>
              Err(panic) => Poll::Ready(Err(panic)),
          }
      }).await;
      match outcome {
          Ok(result) => result,                       // the chain's own Result<()>
          Err(panic) => {
              let message = panic_message(panic);     // KEEP the existing downcast helper + its tests
              tracing::error!(user = %user, remote = %remote, panic = %message, "handler panicked");
              Err(Error::Panic(message))
          }
      }
  }
  ```
  `RecoverWith` (the callback variant) mirrors this. **Keep `panic_message` and its unit tests verbatim.** Caveat to note in a comment: catching a panic mid-render can leave the client terminal mid-escape-sequence; acceptable (connection is closing). One behavioral change from the old version: a panic now unwinds **through** the server task instead of being isolated on a child task — `catch_unwind` contains it, but if any held data is not `UnwindSafe` you may need more `AssertUnwindSafe`. Validate with the existing recover tests + a manual panicking app.
- **`sftp/core.rs`**: already a `Middleware` (struct impl). Flip `handle(&self, mut session: Session, next)` → `handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>) -> Result<()>`. Body: `session.take_channel()` still works (`Option::take` on `&mut`), run sftp, `Ok(())`; else `next.run(session).await`. Its `SftpHandler` is unrelated (russh-sftp), untouched.

### Examples (`examples/*.rs`)
All apps flip `(mut session: Session) -> Result<Session>` → `(session: &mut Session) -> Result<()>`, and final `session.exit(0)` now returns `Result<()>` (still works as the last expr). Middleware fns flip to `(&mut Session, Next<'_>) -> Result<()>`; `next.run(session)` calls stay. Specifically:
- `echo.rs`, `auth.rs`, `env.rs`, `exec.rs`, `signal.rs`, `subsystem.rs`, `shutdown.rs`, `middleware.rs`: mechanical signature/return flips. (echo's `while let Some(event) = session.next().await` loop stays — raw `Session::next` is unchanged.)
- **`tui.rs`**: full rewrite to the own-the-loop API in §3, **including a server-push demo** (the 1s ticker via `tui.sender()`) so resize + minimal-diff + push are all exercised. Delete the `App for Counter` impl. Keep the same visual (title/counter/help) using `draw(|f| ...)`.

---

## 7. Verification plan (per the `verify` / `run` skills)
1. `cargo +nightly build` then `cargo +nightly clippy --all-targets --all-features` — the crate lints are strict (`pedantic`, `nursery`, `unwrap_used`, `unsafe_code = warn`). The `recover` `Box::pin`+`poll_fn` path is unsafe-free by design.
2. `cargo +nightly test` — `parse_key_event` tests (moved to `key.rs`) and `recover`'s `panic_message` tests must pass.
3. `cargo +nightly run --example tui --features ratatui`, `ssh -p 2222 localhost`:
   - One-key change sends a small diff (byte/packet trace) — regression check that diffing still works.
   - The 1s clock pushes redraws (proves `sender()` wakes the loop).
   - A full resize sends a full screen (correct), and the app keeps rendering at the new size (proves the `Viewport::Fixed` autoresize fix — this is the bug that previously bailed the session on a headless server).
   - Cursor: an app that sets `frame.set_cursor_position` shows the cursor there; otherwise hidden.
4. `cargo +nightly run --example echo` etc. — smoke-test the borrow-model middleware path + `Comment` after-write.

## 8. Risks / gotchas to watch
- **`Send` HRTB fragility:** if a builtin's `async fn` body makes the future not-`Send` for all lifetimes, you'll get "Send is not general enough." Keep the closure blanket exactly as §4 (direct return, no `async` wrapper). For struct middleware use explicit-lifetime `handle<'a>` impls (proto2/proto6 pattern).
- **Defaulted `M` on methods:** `session.tui()` with no other type info is ambiguous; annotate the binding (`let mut tui: Tui = ...`) or turbofish. Document.
- **Alt-screen teardown:** forgetting `tui.close().await?` with `alt_screen()` leaves the client in alt-screen. Default (inline) is forgiving. Consider a debug-assert or doc warning.
- **`recover` unwind scope changed** (in-task `catch_unwind` vs child-task isolation). Re-run recover tests; watch for `UnwindSafe` friction.
- **`BoxFuture` alias** gained a lifetime — grep for other users before changing the crate-level alias.
- **clippy `nursery`/`pedantic`** may flag the new APIs (`must_use`, missing docs/errors sections). Mirror the existing heavy doc-comment style.

## 9. Skills for the next session
- **`run`** — launch the TUI app over SSH to confirm rendering/push/resize.
- **`verify`** — validate server-push redraw, resize, minimal-diff before pushing.
- **`code-review`** / **`simplify`** — review the diff once implemented.

## 10. Key existing files (re-read at start)
- `src/tui/app.rs` — old `App`/`Ratatui`/`run_app` (delete) + `parse_key_event` (move to `key.rs`).
- `src/tui/writer.rs` — `SessionWriter` (keep).
- `src/session/core.rs` — `Session`, `next` (skip-protocol loop), `take_channel` (consuming precedent), `do_exit`, identity getters, `exit`/`abort` (to change).
- `src/middleware/{core,next,erased,chain,mod}.rs`, `src/handler.rs` (delete), `src/server/{core,russh}.rs`, `src/lib.rs` — the β′ merge + borrow flip.
- `src/middleware/builtins/recover.rs` — the `catch_unwind` rework; keep `panic_message` + tests.
- Cargo features: `default = []`, `ratatui` opt-in. `events/` must stay outside the `ratatui` gate.
- Validated prototypes: `/tmp/borrow_proto2.rs` (stable struct-only), `/tmp/borrow_proto6.rs` (nightly full). Recompile with `rustc +nightly --edition 2024 --crate-type lib <file>`.
