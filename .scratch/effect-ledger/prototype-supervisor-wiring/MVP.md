# MVP: the ledger is the bug report (throwaway)

Tests the map's **Motivation**: a ledger reproduces a failure you weren't there for, later and elsewhere, and is cheap enough to leave on. Built on top of the supervisor-wiring prototype ([README.md](README.md)), on the same branch (`prototype/ledger-supervisor-wiring`, uncommitted), with the same Roc nightly on macOS x86_64.

## What's added

| Path | What |
|---|---|
| `basic-cli/src/lib.rs` | hand-written wrappers for `hosted_file_read_utf8` and `hosted_utc_now` |
| `basic-cli/src/ledger_codec.rs` | their codecs, plus round-trip tests (`cargo test --release --lib ledger_codec`) |
| `basic-cli/src/ledger_prototype.rs` | both symbols added to the boundary; `pin_shared` (see Findings) |
| `app/agenda.roc` | the demo app: prints today's agenda (UTC) from a file of `HH:MM title` lines |
| `ledger-on.sh` | always-on recording as a shell function, `ledgered`, which gives every ledger a new name |
| `mvp-test.sh` | the whole test, end to end |
| `libhost-ledger-mvp.a` | the host this was built with |

The bug in `agenda.roc`: it compares times in minutes but subtracts in seconds, so `secs_left` underflows during an item's own minute, and Roc crashes. So the crash needs **both** the user's file and the moment they ran it.

Codecs encode what Roc sees, not the Rust value: an `io::Error` becomes the `IOErr` tag `define_common_io_err!` would build from it (just the tag for the nine named kinds, the message for `Other`). Requests encode the native path as Roc holds it: tag byte, then its bytes.

## Run

```sh
cd basic-cli && cargo build --release --lib && cp target/release/libhost.a platform/targets/x64mac/
cd ../app && ../roc-nightly/roc_nightly-macos_x86_64-2026-09-26-d6267b4/roc build agenda.roc --output=agenda
cd .. && ./mvp-test.sh
```

## Result (verbatim, 2026-10-07)

```
== 1. user: always-on recording (17:37 UTC)
Agenda at 2026-10-07T17:37:47Z
00:01  Early thing  done
[ROC CRASHED] Integer subtraction overflowed
[ledger] recorded crashed: Integer subtraction overflowed (shared) -> …/user/.ledgers/agenda-20261007T173746Z-31631-1536.ledger
exit 1

== 2. control: same tasks.txt, after 17:37 UTC has passed
Agenda at 2026-10-07T17:38:00Z
00:01  Early thing  done
17:37  Submit essay draft  done
23:59  Late thing  in 381 min
exit 0

== 3. elsewhere: the ledger directory only; clean env; no tasks.txt
bug.ledger
Agenda at 2026-10-07T17:37:47Z
00:01  Early thing  done
[ROC CRASHED] Integer subtraction overflowed
[ledger] reproduced: crashed: Integer subtraction overflowed after 4 crossings
exit 0
```

- **Steps 2 and 3 pass.** With the user's own file, a minute later, nothing crashes. With only the ledger directory, in a clean env (`env -i`) and with no `tasks.txt`, replay prints the recorded timestamp and reproduces the crash.
- **Cost** (step 4, over three runs): 6–8 ms per request with recording off, 28–37 ms with it on, so about **25 ms of overhead per request**. One ledger is about 4.7 KB of `spans.jsonl`; the 700 KB binary is stored once per build.
- Codec tests: 5 passed.

## Findings

- **On macOS, executing a new file for the first time costs ~280 ms.** A fresh clone and a fresh copy both pay it, and running the same file again costs ~5 ms. Recording pinned a fresh clone per request and ran the child from it, so the first measurement was **316 ms per request** (50×). Fix, `pin_shared`: clone each build once into `<ledgers>/.pins/` (keyed by device, inode, size and mtime), then hardlink each ledger's `exe` to that copy. The shared copy is still a clone of the original, never a hardlink to it, so overwriting the original in place can't reach it. This amends the pinning order from [What do the ledger's version hashes cover?](../issues/08-ledger-version-hashes.md) (clone, then hardlink, then copy, per ledger).
- **Always-on recording needs a new name for every ledger.** Recording refuses an existing directory, so `ledgered` stamps each name with the UTC time, the PID and a random number. This bears on the map's open "refuse, or number like rr" question for §4.
- The remaining ~25 ms is not broken down. Executing a known inode through a new hardlink path measured 17 ms in one run, so part of it may be another macOS per-path check.

## Not tested

- A second Mac. "Elsewhere" is a clean directory and env on the same machine, so a different macOS version (system libraries) was not tried.
- Handles, codegen, fuzzing, `dbg`/`expect`, resource releases, Linux.
- **Known hole:** a hosted function with no wrapper performs its real effect during replay, undetected. The demo stays within the five wrapped functions.
