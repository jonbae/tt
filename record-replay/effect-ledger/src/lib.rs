//! PROTOTYPE, throwaway: the effect ledger as a library any Rust Roc platform
//! host can link. Unix only. Errors mostly abort with a message.
//!
//! A platform hands the ledger one [`Boundary`] table and touches three places:
//!
//! 1. `rust_main`: [`start`] first (a supervisor returns its exit code and
//!    never runs Roc), then host setup, then [`serve_replay`] if [`replaying`].
//! 2. Every host-to-core call (`main!`, `init!`, `respond!`, ...): [`enter`]
//!    before it, [`Request::leave`] after it.
//! 3. Every hosted function: [`cross`], then either the recorded response
//!    ([`Cross::Replay`]) or the real call followed by [`Cross::record`].
//!
//! Switch: `ROC_LEDGER=record:<dir>`, `replay:<dir>` or `dump:<dir>`. Replay
//! takes `ROC_LEDGER_REQUEST=<trace id prefix>` to pick one request.

mod instance;
mod outcome;
mod otlp;
mod pin;
mod supervisor;
mod wire;

pub use otlp::iso_utc;

use std::cell::Cell;
use std::ffi::OsString;
use std::fs::File;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use wire::Frame;

/// Everything the ledger knows about one platform. Record, replay and dump
/// all read this one table; nothing else in the library is platform-specific.
pub struct Boundary {
    /// Recorded as the `ledger.platform` resource attribute.
    pub platform: &'static str,
    pub hosted: &'static [Hosted],
    pub entries: &'static [Entry],
}

/// One hosted function: a core-to-host crossing.
pub struct Hosted {
    pub symbol: &'static str,
    /// Replay never performs effects. Writes the user would have seen are
    /// emulated by the supervisor from the recorded request, as rr does.
    pub echo: Echo,
    pub show_request: fn(&[u8]) -> String,
    pub show_response: fn(&[u8]) -> String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Echo {
    Nothing,
    Stdout,
    Stderr,
}

/// One host-to-core entry. Each call to it is a request with its own trace.
pub struct Entry {
    pub name: &'static str,
    /// Entries whose latest earlier request replay re-runs first, because this
    /// one consumes what they returned: `respond!` needs `init!`'s context.
    pub after: &'static [&'static str],
    /// Replay child only: call the core with a recorded request and return the
    /// encoded response. Effects inside it are answered from the ledger.
    pub replay: fn(&[u8]) -> Vec<u8>,
    /// Marks the root span OK or ERROR in standard OTLP status.
    pub ok: fn(&[u8]) -> bool,
    pub show_request: fn(&[u8]) -> String,
    pub show_response: fn(&[u8]) -> String,
}

impl Boundary {
    fn hosted(&self, symbol: &str) -> Option<&Hosted> {
        self.hosted.iter().find(|h| h.symbol == symbol)
    }

    fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

const CHILD_ENV: &str = "ROC_LEDGER_CHILD";

static BOUNDARY: OnceLock<&'static Boundary> = OnceLock::new();
static LINK: OnceLock<Link> = OnceLock::new();

fn boundary() -> &'static Boundary {
    BOUNDARY.get().expect("effect_ledger::start was not called")
}

fn die(code: i32, msg: &str) -> ! {
    eprintln!("[ledger] {msg}");
    std::process::exit(code)
}

/// Call first in `rust_main`. `Some(code)`: this process was a supervisor and
/// is done. `None`: run the host, plainly or as a ledger child.
pub fn start(boundary: &'static Boundary, process_args: &[OsString]) -> Option<i32> {
    let _ = BOUNDARY.set(boundary);
    if let Some(spec) = std::env::var_os(CHILD_ENV) {
        // Before any Roc code runs, so the app sees the env it would see
        // without a ledger.
        std::env::remove_var(CHILD_ENV);
        become_child(&spec.to_string_lossy());
        return None;
    }
    let spec = std::env::var_os("ROC_LEDGER")?.to_string_lossy().into_owned();
    let usage = "ROC_LEDGER must be record:<dir>, replay:<dir> or dump:<dir>";
    let (mode, dir) = spec.split_once(':').unwrap_or_else(|| die(64, usage));
    let dir = PathBuf::from(dir);
    Some(match mode {
        "record" => supervisor::record(boundary, &dir, process_args),
        "replay" => supervisor::replay(&dir, process_args),
        "dump" => supervisor::dump(&dir, process_args),
        // Internal: one instance, run by that instance's pinned binary.
        "replay-instance" => supervisor::replay_instance(boundary, &dir, process_args),
        "dump-instance" => supervisor::dump_instance(boundary, &dir, process_args),
        _ => die(64, usage),
    })
}

/// A replay child: after host setup, hand the process to [`serve_replay`].
pub fn replaying() -> bool {
    LINK.get().is_some_and(|l| l.replaying)
}

// ---------------------------------------------------------------- child side

struct Link {
    replaying: bool,
    to_sup: Mutex<File>,
    from_sup: Option<Mutex<File>>,
    next_rid: AtomicU64,
}

/// The request this thread is handling. Roc calls are synchronous, so every
/// crossing a request makes happens on the thread that entered it. Keeping the
/// sequence number here, not in one global counter, is what lets concurrent
/// requests share a process without sharing a sequence.
#[derive(Clone, Copy)]
struct Current {
    rid: u64,
    seq: u64,
}

thread_local! {
    static CURRENT: Cell<Option<Current>> = const { Cell::new(None) };
}

fn become_child(spec: &str) {
    // "record:<wfd>" or "replay:<wfd>:<rfd>"
    let parts: Vec<&str> = spec.split(':').collect();
    let fd = |s: &str| -> File {
        let fd: i32 = s.parse().unwrap_or_else(|_| die(64, "bad ROC_LEDGER_CHILD"));
        // Back to close-on-exec, so a grandchild never holds the pipe open.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        unsafe { File::from_raw_fd(fd) }
    };
    let link = match parts.as_slice() {
        ["record", w] => Link { replaying: false, to_sup: Mutex::new(fd(w)), from_sup: None, next_rid: AtomicU64::new(1) },
        ["replay", w, r] => {
            // Ctrl-C reaches only the supervisor (rr keeps one process group).
            supervisor::block_signals(&[libc::SIGINT, libc::SIGQUIT]);
            Link { replaying: true, to_sup: Mutex::new(fd(w)), from_sup: Some(Mutex::new(fd(r))), next_rid: AtomicU64::new(1) }
        }
        _ => die(64, "bad ROC_LEDGER_CHILD"),
    };
    let _ = LINK.set(link);
    std::panic::set_hook(Box::new(|info| {
        send(Frame::Panic { rid: current_rid(), message: info.to_string() });
        eprintln!("{info}");
    }));
}

fn current_rid() -> u64 {
    CURRENT.with(|c| c.get()).map_or(0, |c| c.rid)
}

fn send(frame: Frame) {
    let Some(link) = LINK.get() else { return };
    let mut w = link.to_sup.lock().unwrap_or_else(|p| p.into_inner());
    if frame.write(&mut *w).is_err() {
        // The supervisor is gone; nothing may happen unrecorded.
        unsafe { libc::_exit(1) }
    }
}

fn receive() -> Option<Frame> {
    let link = LINK.get()?;
    let mut r = link.from_sup.as_ref()?.lock().unwrap_or_else(|p| p.into_inner());
    Frame::read(&mut *r).ok().flatten()
}

/// A request in progress on this thread. Dropping it without [`leave`] means
/// the entry never returned: the supervisor works out the outcome.
///
/// [`leave`]: Request::leave
#[must_use]
pub struct Request(Option<u64>);

/// Before a host-to-core call. Only recording does anything here: in replay
/// the supervisor drives every entry, and without a ledger nothing happens.
pub fn enter(entry: &'static str, request: impl FnOnce() -> Vec<u8>) -> Request {
    let Some(link) = LINK.get().filter(|l| !l.replaying) else { return Request(None) };
    let rid = link.next_rid.fetch_add(1, Ordering::SeqCst);
    send(Frame::Enter { rid, entry: entry.to_string(), request: request() });
    CURRENT.with(|c| c.set(Some(Current { rid, seq: 0 })));
    Request(Some(rid))
}

impl Request {
    /// After the host-to-core call returned, with what it returned.
    pub fn leave(self, response: impl FnOnce() -> Vec<u8>) {
        if let Some(rid) = self.0 {
            send(Frame::Leave { rid, response: response() });
            CURRENT.with(|c| c.set(None));
        }
    }
}

/// What a hosted function does at a crossing.
#[must_use]
pub enum Cross {
    /// No ledger: perform the effect.
    Off,
    /// Perform the effect, then [`record`](Cross::record) its response.
    Record { rid: u64, seq: u64 },
    /// Don't perform the effect: here is the recorded response. The wrapper
    /// still releases its arguments, as the real call would have.
    Replay(Vec<u8>),
}

/// At the top of a hosted function, before anything releases its arguments.
pub fn cross(symbol: &'static str, request: impl FnOnce() -> Vec<u8>) -> Cross {
    let Some(link) = LINK.get() else { return Cross::Off };
    let Some(mut current) = CURRENT.with(|c| c.get()) else {
        die(70, &format!("{symbol} crossed outside any request: an entry into the core isn't wrapped"));
    };
    current.seq += 1;
    CURRENT.with(|c| c.set(Some(current)));
    let Current { rid, seq } = current;
    send(Frame::Begin { rid, seq, symbol: symbol.to_string(), request: request() });
    if !link.replaying {
        return Cross::Record { rid, seq };
    }
    match receive() {
        Some(Frame::Respond { death, response }) => {
            // Killed or exited between crossings in the recording: go the
            // same way as soon as the last recorded crossing returns.
            if let Some(d) = death {
                d.carry_out();
            }
            Cross::Replay(response)
        }
        Some(Frame::Die { death }) => death.carry_out(),
        // The supervisor is gone or halted us at a divergence.
        _ => unsafe { libc::_exit(1) },
    }
}

impl Cross {
    pub fn record(self, response: impl FnOnce() -> Vec<u8>) {
        if let Cross::Record { rid, seq } = self {
            send(Frame::End { rid, seq, response: response() });
        }
    }
}

/// From the platform's `roc_crashed`, before it exits.
pub fn crashed(message: &[u8]) {
    send(Frame::Crashed { rid: current_rid(), message: String::from_utf8_lossy(message).into_owned() });
}

/// From the platform's `roc_expect_failed`.
pub fn expect_failed(message: &[u8]) {
    send(Frame::Expect { rid: current_rid(), message: String::from_utf8_lossy(message).into_owned() });
}

/// A replay child's whole life: run each request the supervisor sends through
/// its entry's `replay`, report what it returned, exit when the pipe closes.
pub fn serve_replay() -> i32 {
    loop {
        let Some(Frame::Run { rid, entry, request }) = receive() else { return 0 };
        let e = boundary().entry(&entry).unwrap_or_else(|| die(70, &format!("no entry {entry}")));
        CURRENT.with(|c| c.set(Some(Current { rid, seq: 0 })));
        let response = (e.replay)(&request);
        send(Frame::Leave { rid, response });
        CURRENT.with(|c| c.set(None));
    }
}
