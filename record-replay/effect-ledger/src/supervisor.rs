//! The supervisor: a process that never runs Roc. Record writes the ledger,
//! replay answers its child from it, dump prints it.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};

use serde_json::Value;

use crate::instance::{Instance, PINNED, SPANS};
use crate::otlp::{attr_int, attr_str, b64, iso_utc, now_nanos, Span, Trace};
use crate::outcome::{Death, InFlight, Outcome};
use crate::pin::{current_exe, pin_shared};
use crate::wire::Frame;
use crate::{die, Boundary, Echo, CHILD_ENV};

// ---------------------------------------------------------------- signals

static CHILD_PID: AtomicI32 = AtomicI32::new(0);
static INTERRUPTED: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_sigterm(_: i32) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
            libc::alarm(5);
        }
    }
}

extern "C" fn on_sigalrm(_: i32) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

extern "C" fn on_replay_sigint(_: i32) {
    INTERRUPTED.store(1, Ordering::SeqCst);
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

fn handle(sig: i32, f: extern "C" fn(i32)) {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = f as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(sig, &sa, std::ptr::null_mut());
    }
}

pub fn block_signals(sigs: &[i32]) {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in sigs {
            libc::sigaddset(&mut set, *s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

pub fn die_by_signal(sig: i32) -> ! {
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        libc::raise(sig);
        libc::_exit(128 + sig)
    }
}

/// Exit the way the child did, so the shell sees the same thing.
fn exit_like(status: ExitStatus) -> i32 {
    if let Some(sig) = status.signal() {
        die_by_signal(sig);
    }
    status.code().unwrap_or(1)
}

fn death_of(status: ExitStatus) -> Death {
    match status.signal() {
        Some(sig) => Death::Signal(sig),
        None => Death::Exit(status.code().unwrap_or(-1)),
    }
}

fn pipe() -> (File, File) {
    let mut fds = [0i32; 2];
    unsafe { libc::pipe(fds.as_mut_ptr()) };
    for fd in fds {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
}

fn child_command(exe: &Path, args: &[OsString], env_spec: String, keep_fds: Vec<i32>, block: Vec<i32>) -> Command {
    let mut cmd = Command::new(exe);
    if let Some(argv0) = args.first() {
        cmd.arg0(argv0);
    }
    cmd.args(args.iter().skip(1)).env_remove("ROC_LEDGER").env(CHILD_ENV, env_spec);
    unsafe {
        cmd.pre_exec(move || {
            for fd in &keep_fds {
                libc::fcntl(*fd, libc::F_SETFD, 0);
            }
            // The supervisor ignores these; ignored dispositions survive
            // exec, so put the defaults back for the app.
            for s in [libc::SIGINT, libc::SIGQUIT, libc::SIGHUP, libc::SIGTERM, libc::SIGALRM] {
                libc::signal(s, libc::SIG_DFL);
            }
            block_signals(&block);
            Ok(())
        });
    }
    cmd
}

fn become_pinned(exe: &Path, args: &[OsString]) {
    if !same_file::is_same_file(current_exe(), exe).unwrap_or(false) {
        let mut cmd = Command::new(exe);
        if let Some(argv0) = args.first() {
            cmd.arg0(argv0);
        }
        let err = cmd.args(args.iter().skip(1)).exec();
        die(71, &format!("cannot exec the pinned binary: {err}"));
    }
}

// ---------------------------------------------------------------- record

struct Open {
    trace: Trace,
    entry: String,
    request: Vec<u8>,
    start: u128,
    in_flight: Option<(InFlight, u128)>,
    /// The crash message, and when it arrived: before the exit it causes.
    crashed: Option<(String, u128)>,
}

/// Records this process as a new instance of the ledger at `ledger`.
pub fn record(b: &Boundary, ledger: &Path, args: &[OsString]) -> i32 {
    let instance = Instance::create(ledger);
    // Pin first, then run the child from the pinned copy.
    let exe = instance.dir.join(PINNED);
    let how = pin_shared(&current_exe(), &exe);
    let mut spans = File::create(instance.dir.join(SPANS)).unwrap();
    let service = args.first().and_then(|a| Path::new(a).file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    let (mut from_child, to_sup) = pipe();
    let wfd = to_sup.as_raw_fd();
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
    handle(libc::SIGTERM, on_sigterm);
    handle(libc::SIGALRM, on_sigalrm);
    let started = now_nanos();
    let mut child = child_command(&exe, args, format!("record:{wfd}"), vec![wfd], vec![])
        .spawn()
        .unwrap_or_else(|e| die(71, &format!("cannot start the pinned binary: {e}")));
    CHILD_PID.store(child.id() as i32, Ordering::SeqCst);
    drop(to_sup);
    let resource = serde_json::json!({"attributes": [
        attr_str("service.name", &service),
        attr_str("service.instance.id", &instance.id),
        attr_int("process.pid", child.id() as i64),
        attr_str("ledger.pinned", how),
        attr_str("ledger.platform", b.platform),
    ]});
    let mut meta = serde_json::json!({
        "service.instance.id": instance.id,
        "service.name": service,
        "ledger.platform": b.platform,
        "process.pid": child.id(),
        "started": started.to_string(),
    });
    instance.write_meta(&meta);

    let mut open: HashMap<u64, Open> = HashMap::new();
    let mut ended: Vec<(String, String, String, bool)> = Vec::new();
    let mut write_root = |spans: &mut File, o: Open, outcome: Outcome, end: u128| {
        let entry = b.entry(&o.entry);
        let ok = matches!((&outcome, entry), (Outcome::Returned(r), Some(e)) if (e.ok)(r));
        let described = outcome.describe(entry);
        spans.write_all(o.trace.root(&resource, &o.entry, o.start, end, &o.request, &outcome, ok, &described).as_bytes()).unwrap();
        ended.push((o.trace.trace_id, o.entry, described, matches!(outcome, Outcome::Returned(_))));
    };
    loop {
        let frame = match Frame::read(&mut from_child) {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => die(74, &format!("pipe: {e}")),
        };
        match frame {
            Frame::Enter { rid, entry, request } => {
                open.insert(rid, Open { trace: Trace::new(), entry, request, start: now_nanos(), in_flight: None, crashed: None });
            }
            Frame::Begin { rid, seq, symbol, request } => {
                let o = open.get_mut(&rid).expect("BEGIN outside a request");
                o.in_flight = Some((InFlight { seq, effect: symbol, request }, now_nanos()));
            }
            Frame::End { rid, seq, response } => {
                let o = open.get_mut(&rid).expect("END outside a request");
                let (f, t0) = o.in_flight.take().expect("END without BEGIN");
                assert_eq!(f.seq, seq, "END for the wrong crossing");
                // Whole lines only: a line is written once its crossing ends.
                spans.write_all(o.trace.crossing(&resource, seq, &f.effect, t0, now_nanos(), &f.request, Some(&response)).as_bytes()).unwrap();
            }
            Frame::Leave { rid, response } => {
                let o = open.remove(&rid).expect("LEAVE outside a request");
                write_root(&mut spans, o, Outcome::Returned(response), now_nanos());
            }
            Frame::Crashed { rid, message } => match open.get_mut(&rid) {
                Some(o) => o.crashed = Some((message, now_nanos())),
                None => eprintln!("[ledger] crash outside any request: {message}"),
            },
            Frame::Expect { message, .. } => eprintln!("[ledger] expect failed: {message}"),
            Frame::Panic { message, .. } => eprintln!("[ledger] host panic: {message}"),
            _ => {}
        }
    }
    let status = child.wait().unwrap();
    // Requests still open when the process died: one crashed, the others went
    // down with the process.
    let mut rids: Vec<u64> = open.keys().copied().collect();
    rids.sort();
    for rid in rids {
        let o = open.remove(&rid).unwrap();
        let (outcome, end) = match o.crashed.clone() {
            Some((m, at)) => (Outcome::Crashed(m), at),
            None => (Outcome::Died { death: death_of(status), in_flight: o.in_flight.as_ref().map(|(f, _)| f.clone()) }, now_nanos()),
        };
        if let Some((f, t0)) = &o.in_flight {
            spans.write_all(o.trace.crossing(&resource, f.seq, &f.effect, *t0, now_nanos(), &f.request, None).as_bytes()).unwrap();
        }
        write_root(&mut spans, o, outcome, end);
    }
    drop(spans);
    let mut exit = death_of(status).to_json();
    exit["at"] = now_nanos().to_string().into();
    meta["exit"] = exit;
    instance.write_meta(&meta);
    eprintln!("[ledger] recorded {} request(s) ({how}) -> {}, instance {}", ended.len(), ledger.display(), instance.id);
    for (id, entry, described, returned) in &ended {
        if ended.len() == 1 || !returned {
            eprintln!("[ledger]   {entry} {}: {described}", &id[..8]);
        }
    }
    exit_like(status)
}

// ---------------------------------------------------------------- reading a ledger

struct Recorded {
    trace_id: String,
    entry: String,
    request: Vec<u8>,
    outcome: Outcome,
    start: u128,
    crossings: Vec<Span>,
}

impl Recorded {
    fn label(&self) -> String {
        format!("{} {}", self.entry, &self.trace_id[..8])
    }
}

fn refuse(msg: &str) -> ! {
    die(65, &format!("refused: {msg}"))
}

/// Strict: anything replay can't trust is refused up front (exit 65).
fn load(b: &Boundary, dir: &Path) -> Vec<Recorded> {
    let text = fs::read_to_string(dir.join(SPANS)).unwrap_or_else(|_| refuse("no spans.jsonl"));
    let mut order: Vec<String> = Vec::new();
    let mut roots: HashMap<String, Span> = HashMap::new();
    let mut crossings: HashMap<String, Vec<Span>> = HashMap::new();
    for (n, line) in text.lines().enumerate() {
        let span = Span::parse(line).unwrap_or_else(|why| refuse(&format!("line {}: {why}", n + 1)));
        if !order.contains(&span.trace_id) {
            order.push(span.trace_id.clone());
        }
        if span.seq == 0 {
            if b.entry(&span.effect).is_none() {
                refuse(&format!("line {}: {} is not an entry of {}", n + 1, span.effect, b.platform));
            }
            roots.insert(span.trace_id.clone(), span);
        } else if span.answered {
            if b.hosted(&span.effect).is_none() {
                refuse(&format!("seq {} names {}, which is outside the boundary", span.seq, span.effect));
            }
            crossings.entry(span.trace_id.clone()).or_default().push(span);
        }
    }
    let mut requests = Vec::new();
    for id in order {
        let root = roots.remove(&id).unwrap_or_else(|| refuse(&format!("incomplete: request {} has no root span", &id[..8])));
        let outcome = Outcome::from_json(&root.response).unwrap_or_else(|| refuse(&format!("request {}: unreadable outcome", &id[..8])));
        let mut cs = crossings.remove(&id).unwrap_or_default();
        cs.sort_by_key(|c| c.seq);
        for (i, c) in cs.iter().enumerate() {
            if c.seq != i as u64 + 1 {
                refuse(&format!("request {}: gap or duplicate at seq {}", &id[..8], c.seq));
            }
        }
        requests.push(Recorded { trace_id: id, entry: root.effect, request: root.request, outcome, start: root.start, crossings: cs });
    }
    requests.sort_by_key(|r| r.start);
    requests
}

// ---------------------------------------------------------------- replay

enum Verdict {
    Reproduced,
    Diverged(String),
    Interrupted,
    Panicked(String),
}

/// Every request in the ledger at `path`, or the one `ROC_LEDGER_REQUEST`
/// names. Each instance replays in its own pinned binary, so instances
/// recorded by different builds each read their own ledger entries.
pub fn replay(path: &Path, args: &[OsString]) -> i32 {
    let instances = Instance::list(path);
    if instances.is_empty() {
        refuse(&format!("no instances in {}", path.display()));
    }
    let chosen: Vec<&Instance> = match std::env::var("ROC_LEDGER_REQUEST") {
        Ok(prefix) => {
            let hits: Vec<&Instance> = instances.iter().flat_map(|i| request_ids(i).into_iter().filter(|id| id.starts_with(&prefix)).map(move |_| i)).collect();
            if hits.len() != 1 {
                die(64, &format!("ROC_LEDGER_REQUEST={prefix} matches {} requests", hits.len()));
            }
            hits
        }
        Err(_) => instances.iter().collect(),
    };
    // Ctrl-C is the instance replay's to handle.
    unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
    let mut result = 0;
    for i in chosen {
        if instances.len() > 1 {
            eprintln!("[ledger] instance {}", i.id);
        }
        let code = match in_instance(i, "replay-instance", args).status() {
            Ok(status) => status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
            Err(e) => {
                eprintln!("[ledger] refused: instance {} has no runnable pinned binary: {e}", i.id);
                65
            }
        };
        if code == 130 {
            return 130;
        }
        if result == 0 {
            result = code;
        }
    }
    result
}

/// `mode` on one instance, run by that instance's pinned binary.
fn in_instance(i: &Instance, mode: &str, args: &[OsString]) -> Command {
    let mut cmd = Command::new(i.dir.join(PINNED));
    if let Some(argv0) = args.first() {
        cmd.arg0(argv0);
    }
    cmd.args(args.iter().skip(1)).env("ROC_LEDGER", format!("{mode}:{}", i.dir.display()));
    cmd
}

/// The requests an instance's ledger entries belong to, by trace id, in file order.
fn request_ids(i: &Instance) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for s in fs::read_to_string(i.dir.join(SPANS)).unwrap_or_default().lines().filter_map(|l| Span::parse(l).ok()) {
        if !ids.contains(&s.trace_id) {
            ids.push(s.trace_id);
        }
    }
    ids
}

pub fn replay_instance(b: &Boundary, dir: &Path, args: &[OsString]) -> i32 {
    let exe = dir.join(PINNED);
    if !exe.exists() {
        refuse("incomplete: no pinned binary");
    }
    // Supervisor and child must be one build: always exec the pinned binary.
    become_pinned(&exe, args);
    let requests = load(b, dir);
    let targets: Vec<usize> = match std::env::var("ROC_LEDGER_REQUEST") {
        Ok(prefix) => {
            let hits: Vec<usize> = (0..requests.len()).filter(|&i| requests[i].trace_id.starts_with(&prefix)).collect();
            if hits.len() != 1 {
                die(64, &format!("ROC_LEDGER_REQUEST={prefix} matches {} requests", hits.len()));
            }
            hits
        }
        Err(_) => (0..requests.len()).collect(),
    };
    unsafe {
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
    handle(libc::SIGINT, on_replay_sigint);

    let mut diverged = 0;
    for &t in &targets {
        let r = &requests[t];
        let chain = chain_for(b, &requests, t);
        match replay_chain(b, &exe, args, &requests, &chain) {
            Verdict::Reproduced => eprintln!(
                "[ledger] reproduced {}: {} after {} crossings",
                r.label(),
                r.outcome.describe(b.entry(&r.entry)),
                r.crossings.len()
            ),
            Verdict::Diverged(d) => {
                diverged += 1;
                eprintln!("[ledger] divergence in {}: {d}", r.label());
                eprintln!("[ledger]   ledger: {}", dir.display());
                eprintln!("[ledger] This is a bug in the effect ledger: the pinned build replayed its own recording differently.");
            }
            Verdict::Interrupted => {
                eprintln!("[ledger] interrupted during {}", r.label());
                return 130;
            }
            Verdict::Panicked(p) => {
                eprintln!("[ledger] replay bug: Rust panic in the child: {p}");
                return 70;
            }
        }
    }
    if diverged > 0 {
        70
    } else {
        0
    }
}

/// The requests a replay of `t` runs, in order: those its entry consumes, then `t`.
fn chain_for(b: &Boundary, requests: &[Recorded], t: usize) -> Vec<usize> {
    let entry = b.entry(&requests[t].entry).unwrap();
    let mut chain = Vec::new();
    for name in entry.after {
        match (0..t).rev().find(|&i| requests[i].entry == *name) {
            Some(i) => chain.push(i),
            None => refuse(&format!("{} needs an earlier {name}, and the ledger has none", requests[t].label())),
        }
    }
    chain.push(t);
    chain
}

fn replay_chain(b: &Boundary, exe: &Path, args: &[OsString], requests: &[Recorded], chain: &[usize]) -> Verdict {
    let (mut from_child, to_sup) = pipe();
    let (from_sup, mut to_child) = pipe();
    let (wfd, rfd) = (to_sup.as_raw_fd(), from_sup.as_raw_fd());
    let mut child = child_command(exe, args, format!("replay:{wfd}:{rfd}"), vec![wfd, rfd], vec![libc::SIGINT, libc::SIGQUIT])
        .stdin(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| die(71, &format!("cannot start the pinned binary: {e}")));
    CHILD_PID.store(child.id() as i32, Ordering::SeqCst);
    drop((to_sup, from_sup));
    let kill = |c: &Child| unsafe {
        libc::kill(c.id() as i32, libc::SIGKILL);
    };

    let mark = std::env::var_os("ROC_LEDGER_MARK").is_some(); // stands in for rr's -M
    let target = chain.len() - 1;
    let mut crashed: Option<String> = None;
    let mut panicked: Option<String> = None;
    let mut divergence: Option<String> = None;
    let mut diverged_in = 0;
    // The request the child was in when it stopped before leaving it.
    let mut unfinished: Option<(usize, u64, Option<InFlight>)> = None;

    'chain: for (i, &ri) in chain.iter().enumerate() {
        let r = &requests[ri];
        let run = Frame::Run { rid: i as u64 + 1, entry: r.entry.clone(), request: r.request.clone() };
        if run.write(&mut to_child).is_err() {
            unfinished = Some((i, 0, None));
            break;
        }
        let mut matched = 0u64;
        let mut sent: Option<InFlight> = None;
        diverged_in = i;
        loop {
            let frame = match Frame::read(&mut from_child) {
                Ok(Some(f)) => f,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                _ => {
                    unfinished = Some((i, matched, sent));
                    break 'chain;
                }
            };
            match frame {
                Frame::Begin { seq, symbol, request, .. } => {
                    if let Outcome::Died { death, in_flight: Some(f) } = &r.outcome {
                        if f.seq == seq {
                            if f.effect != symbol || f.request != request {
                                divergence = Some(format!("Mismatch at seq {seq} ({symbol})"));
                                kill(&child);
                                break 'chain;
                            }
                            let _ = Frame::Die { death: *death }.write(&mut to_child);
                            sent = Some(f.clone());
                            continue;
                        }
                    }
                    let Some(e) = r.crossings.get(seq as usize - 1) else {
                        divergence = Some(format!("Extra at seq {seq} ({symbol})"));
                        kill(&child);
                        break 'chain;
                    };
                    if e.effect != symbol || e.request != request {
                        let show = |s: &str, req: &[u8]| b.hosted(s).map_or(format!("{req:?}"), |h| (h.show_request)(req));
                        divergence = Some(format!(
                            "Mismatch at seq {seq} ({symbol})\n[ledger]   recorded: {}({})\n[ledger]   replayed: {symbol}({})",
                            e.effect,
                            show(&e.effect, &e.request),
                            show(&symbol, &request)
                        ));
                        kill(&child);
                        break 'chain;
                    }
                    // Test hook only: slow replay down so Ctrl-C can land mid-replay.
                    if let Some(ms) = std::env::var("ROC_LEDGER_SLOW_MS").ok().and_then(|v| v.parse().ok()) {
                        std::thread::sleep(std::time::Duration::from_millis(ms));
                    }
                    if i == target {
                        echo(b, &symbol, &request, seq, mark);
                    }
                    // Died between crossings: the death rides on the last answer.
                    let death = match &r.outcome {
                        Outcome::Died { death, in_flight: None } if seq as usize == r.crossings.len() => Some(*death),
                        _ => None,
                    };
                    let _ = Frame::Respond { death, response: e.response.clone() }.write(&mut to_child);
                    matched = seq;
                }
                Frame::Leave { response, .. } => {
                    if (matched as usize) < r.crossings.len() {
                        let e = &r.crossings[matched as usize];
                        divergence = Some(format!("Missing at seq {} ({})", e.seq, e.effect));
                        kill(&child);
                        break 'chain;
                    }
                    let replayed = Outcome::Returned(response);
                    if replayed != r.outcome {
                        divergence = Some(outcome_mismatch(b, r, &replayed));
                        kill(&child);
                        break 'chain;
                    }
                    continue 'chain;
                }
                Frame::Crashed { message, .. } => crashed = Some(message),
                Frame::Expect { message, .. } => eprintln!("[ledger] expect failed: {message}"),
                Frame::Panic { message, .. } => panicked = Some(message),
                _ => {}
            }
        }
    }
    // Closing the pipe tells a child that left every request to exit.
    drop(to_child);
    let status = child.wait().unwrap();

    if INTERRUPTED.load(Ordering::SeqCst) == 1 {
        return Verdict::Interrupted;
    }
    if let Some(p) = panicked {
        return Verdict::Panicked(p);
    }
    if let Some(d) = divergence {
        return Verdict::Diverged(in_prerequisite(requests, chain, diverged_in, d));
    }
    let Some((i, matched, sent)) = unfinished else { return Verdict::Reproduced };
    let r = &requests[chain[i]];
    if sent.is_none() && (matched as usize) < r.crossings.len() {
        let e = &r.crossings[matched as usize];
        return Verdict::Diverged(in_prerequisite(requests, chain, i, format!("Missing at seq {} ({})", e.seq, e.effect)));
    }
    let replayed = match crashed {
        Some(m) => Outcome::Crashed(m),
        None => Outcome::Died { death: death_of(status), in_flight: sent },
    };
    if replayed != r.outcome {
        return Verdict::Diverged(in_prerequisite(requests, chain, i, outcome_mismatch(b, r, &replayed)));
    }
    if i != target {
        return Verdict::Diverged(format!("{} ended the process before the request ran", r.label()));
    }
    Verdict::Reproduced
}

fn in_prerequisite(requests: &[Recorded], chain: &[usize], i: usize, d: String) -> String {
    if i + 1 == chain.len() {
        d
    } else {
        format!("while replaying {} first: {d}", requests[chain[i]].label())
    }
}

fn outcome_mismatch(b: &Boundary, r: &Recorded, replayed: &Outcome) -> String {
    let entry = b.entry(&r.entry);
    format!(
        "Mismatch at seq 0 ({})\n[ledger]   recorded: {}\n[ledger]   replayed: {}",
        r.entry,
        r.outcome.describe(entry),
        replayed.describe(entry)
    )
}

/// Replay emulates writes in the supervisor, as rr does; the child performs
/// nothing. An echoing hosted function's request is the line it writes.
fn echo(b: &Boundary, symbol: &str, request: &[u8], seq: u64, mark: bool) {
    let tag = if mark { format!("[seq {seq}] ") } else { String::new() };
    let out: Box<dyn Write> = match b.hosted(symbol).map(|h| h.echo) {
        Some(Echo::Stdout) => Box::new(io::stdout().lock()),
        Some(Echo::Stderr) => Box::new(io::stderr().lock()),
        _ => return,
    };
    let mut out = out;
    let _ = out.write_all(tag.as_bytes());
    let _ = out.write_all(request);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

// ---------------------------------------------------------------- dump

/// Every instance's requests merged into one timeline, the way a server's log
/// reads: each row tagged with its instance and request, and each process's
/// start and exit as rows of their own. `ROC_LEDGER_REQUEST=<prefix>` keeps
/// only the matching requests' rows. Each instance's pinned binary decodes its
/// own rows, since instances may be different builds.
pub fn dump(path: &Path, args: &[OsString]) -> i32 {
    let instances = Instance::list(path);
    if instances.is_empty() {
        die(66, &format!("no instances in {}", path.display()));
    }
    let only = std::env::var("ROC_LEDGER_REQUEST").ok();
    let matching = |id: &String| only.as_deref().map_or(true, |p| id.starts_with(p));
    let ids: Vec<Vec<String>> = instances.iter().map(request_ids).collect();
    let total: usize = ids.iter().map(Vec::len).sum();
    let shown = ids.iter().flatten().filter(|id| matching(id)).count();
    if only.is_some() && shown == 0 {
        die(64, &format!("ROC_LEDGER_REQUEST={} matches no request", only.unwrap_or_default()));
    }

    let mut result = 0;
    let mut rows: Vec<String> = Vec::new();
    for (i, ids) in instances.iter().zip(&ids) {
        if only.is_some() && !ids.iter().any(matching) {
            continue;
        }
        let meta = i.meta();
        let process_row = |at: &Value, step: &str, what: String| {
            let at = at.as_str().and_then(|t| t.parse().ok()).unwrap_or(0);
            format!("{}  {:<4}  {:8}  {step:>5}  {what}", iso_utc(at), i.short(), "")
        };
        if only.is_none() && !meta["started"].is_null() {
            let s = |k: &str| meta[k].as_str().unwrap_or("?").to_string();
            rows.push(process_row(&meta["started"], "start", format!("{} ({}), pid {}", s("service.name"), s("ledger.platform"), meta["process.pid"])));
        }
        match in_instance(i, "dump-instance", args).stderr(Stdio::inherit()).output() {
            Ok(out) if out.status.success() => rows.extend(String::from_utf8_lossy(&out.stdout).lines().map(String::from)),
            Ok(out) => {
                eprintln!("[ledger] instance {}: dump failed ({})", i.id, out.status);
                result = 66;
            }
            Err(e) => {
                eprintln!("[ledger] instance {}: no runnable pinned binary, so nothing can decode it: {e}", i.id);
                result = 66;
            }
        }
        if only.is_none() && !meta["exit"].is_null() {
            let how = Death::from_json(&meta["exit"]).map_or("exited".into(), Death::describe);
            rows.push(process_row(&meta["exit"]["at"], "exit", how));
        }
    }
    // ISO times sort as text. The sort is stable, so a process's start stays
    // ahead of a first row stamped in the same microsecond.
    rows.sort_by(|a, b| a.get(..27).cmp(&b.get(..27)));

    println!("ledger     {}", path.display());
    println!("service    {}", instances[0].meta()["service.name"].as_str().unwrap_or("?"));
    println!("instances  {}", instances.len());
    match &only {
        Some(p) => println!("requests   {shown} of {total}, matching {p}"),
        None => println!("requests   {total}"),
    }
    println!();
    println!("{:<27}  {:<4}  {:<8}  {:>5}", "time", "inst", "request", "seq");
    for r in &rows {
        println!("{r}");
    }
    result
}

/// One instance's rows for [`dump`], decoded by the instance's own build.
/// Lenient where replay refuses: problems go to stderr, not fatal.
pub fn dump_instance(b: &Boundary, dir: &Path, args: &[OsString]) -> i32 {
    let exe = dir.join(PINNED);
    if !exe.exists() {
        die(66, "no pinned binary, so nothing here can decode the payloads");
    }
    become_pinned(&exe, args);
    let text = fs::read_to_string(dir.join(SPANS)).unwrap_or_else(|_| die(66, "no spans.jsonl"));

    let mut spans = Vec::new();
    for (n, line) in text.lines().enumerate() {
        match Span::parse(line) {
            Ok(s) => spans.push(s),
            Err(why) => eprintln!("[ledger] {} line {}: {why}", dir.display(), n + 1),
        }
    }
    let only = std::env::var("ROC_LEDGER_REQUEST").ok();
    let shown = |id: &str| only.as_deref().map_or(true, |p| id.starts_with(p));
    let roots: HashMap<&str, &Span> = spans.iter().filter(|s| s.seq == 0).map(|s| (s.trace_id.as_str(), s)).collect();

    // (at, request, rank within the request, step, what happened)
    let mut rows: Vec<(u128, &str, u64, String, String)> = Vec::new();
    for s in spans.iter().filter(|s| shown(&s.trace_id)) {
        let id = &s.trace_id[..8];
        if s.seq == 0 {
            let entry = b.entry(&s.effect);
            let request = entry.map_or_else(|| format!("<{} bytes>", s.request.len()), |e| (e.show_request)(&s.request));
            let described = Outcome::from_json(&s.response).map_or("unreadable outcome".into(), |o| o.describe(entry));
            rows.push((s.start, id, 0, "begin".into(), format!("{}({request})", s.effect)));
            rows.push((s.end, id, u64::MAX, "end".into(), described));
            continue;
        }
        let hosted = b.hosted(&s.effect);
        let request = hosted.map_or_else(|| format!("<{} bytes: {}>", s.request.len(), b64(&s.request)), |h| (h.show_request)(&s.request));
        let response = if s.answered {
            hosted.map_or_else(|| format!("<{} bytes: {}>", s.response.len(), b64(&s.response)), |h| (h.show_response)(&s.response))
        } else {
            match roots.get(s.trace_id.as_str()).and_then(|r| Outcome::from_json(&r.response)) {
                Some(Outcome::Died { death, .. }) => format!("no response: {}", death.describe()),
                _ => "no response".into(),
            }
        };
        rows.push((s.start, id, s.seq, s.seq.to_string(), format!("{}({request}) -> {response}", s.effect)));
    }
    let mut unfinished: Vec<&str> = Vec::new();
    for s in &spans {
        if shown(&s.trace_id) && !roots.contains_key(s.trace_id.as_str()) && !unfinished.contains(&s.trace_id.as_str()) {
            unfinished.push(&s.trace_id);
        }
    }
    for id in unfinished {
        let last = rows.iter().filter(|r| id.starts_with(r.1)).map(|r| r.0).max().unwrap_or(0);
        rows.push((last, &id[..8], u64::MAX, "end".into(), "no root span: the request never finished, or its supervisor was killed".into()));
    }
    rows.sort_by_key(|r| (r.0, r.2));

    let inst = Instance::list(dir).first().map(|i| i.short().to_string()).unwrap_or_default();
    for (at, id, _, step, what) in &rows {
        println!("{}  {inst:<4}  {id}  {step:>5}  {what}", iso_utc(*at));
    }
    0
}
