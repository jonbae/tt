# PROTOTYPE: supervisor wiring (throwaway)

Asset for [What do the supervisor and its child say to each other?](../issues/10-prototype-supervisor-wiring.md). This is hand-written against basic-cli `0.23.0` and built with Roc `nightly-2026-09-26-d6267b4` on macOS x86_64. There is no codegen, and payloads are opaque bytes.

## What's here

| Path | What |
|---|---|
| `basic-cli/` | basic-cli `0.23.0`, branch `prototype/ledger-supervisor-wiring` |
| `basic-cli/src/ledger_prototype.rs` | supervisor (record and replay), child link, frames, OTLP lines, pinning |
| `basic-cli/src/lib.rs` | `rust_main` dispatch; wrappers for `hosted_stdout_line`, `hosted_stdin_line`, and `hosted_stderr_line` (`main_for_host!` reports an `Err` through it); `roc_crashed`/`roc_expect_failed` hooks |
| `app/two_questions.roc` | the app: "Name?", "Age?" |
| `drive.py` | pty driver: Ctrl-C is byte `0x03` on a real pty, so SIGINT hits the whole foreground process group |
| `ledgers/` | the ledgers the scenarios below produced |

## Run

```sh
cd basic-cli && cargo build --release --lib && cp target/release/libhost.a platform/targets/x64mac/
cd ../app && ../roc-nightly/roc_nightly-macos_x86_64-2026-09-26-d6267b4/roc build two_questions.roc --output=two-questions
cd .. && python3 drive.py "wait:Name?,send:Ann,wait:Age?,ctrlc" -- env ROC_LEDGER=record:ledgers/x.ledger app/two-questions
python3 drive.py "" -- env ROC_LEDGER=replay:ledgers/x.ledger app/two-questions
```

## Wiring as built

```
ROC_LEDGER=record:<dir> ./app
  supervisor (./app, never runs Roc)
    pin ./app -> <dir>/exe            clone, else hardlink, else copy
    pick traceId + root spanId        before the child exists
    ignore INT/QUIT/HUP; TERM -> forward, SIGKILL after 5 s
    spawn <dir>/exe, argv0 kept, env ROC_LEDGER_CHILD=record:<wfd>
        pre_exec: clear CLOEXEC on wfd; INT/QUIT/HUP/TERM back to SIG_DFL
    child: strips ROC_LEDGER_CHILD, sets wfd CLOEXEC again, runs Roc
       --BEGIN(seq, symbol, request)-->   supervisor timestamps it
       --END(seq, response)---------->    supervisor writes one span line
       --RETURNED(i32) | CRASHED(msg) | EXPECT(msg) | PANIC(msg)-->
    EOF -> waitpid -> root main! line (args + outcome; in-flight BEGIN joins the outcome)
    exit the way the child did (re-raise its signal)

ROC_LEDGER=replay:<dir> ./anything-ledger-aware
  exec <dir>/exe (same argv, same env)  -> the pinned binary is the supervisor
  refuse (65): no exe, torn line, no root, seq gap, effect outside the boundary
  spawn self with the RECORDED args, stdin=/dev/null,
        ROC_LEDGER_CHILD=replay:<wfd>:<rfd>, SIGINT+SIGQUIT blocked, same pgrp
       <--BEGIN(seq, symbol, request)--   compare with entry seq
       --RESPOND(die_after, response)->   or, at the recorded in-flight seq:
       --DIE(signal)------------------>   child unblocks + raises it itself
    supervisor echoes stdout/stderr requests (child performs nothing)
    Ctrl-C: supervisor SIGKILLs child, "interrupted after seq N", 130
```

## Scenarios (all run, output verbatim)

| # | Scenario | Result |
|---|---|---|
| 2/3 | record, answer both; replay with no stdin | `reproduced: returned 0 after 5 crossings`, exit 0 |
| 4/5 | **record, Ctrl-C at "Age?"; replay** | record: `killed by SIGINT during seq 4 (hosted_stdin_line)`, shell sees SIGINT. Replay prints Name?/Age?, the child dies by SIGINT at seq 4: `reproduced`, exit 0 |
| 6 | ledger tampered at seq 3 | `divergence: Mismatch at seq 3 (hosted_stdout_line)`, exit 70 |
| 7 | root line removed | `refused: incomplete: no main! span`, exit 65 |
| 8 | seq 1 renamed to `hosted_file_delete` | `refused: … outside the boundary`, exit 65 |
| 9 | Ctrl-D at "Age?" | `Program exited with error: MissingAge` via `hosted_stderr_line`; `returned 1`; replay reproduces |
| 10 | SIGTERM to the recording supervisor | forwarded; `killed by SIGTERM during seq 4`; replay reproduces |
| 11 | replay with an **edited** build ("Your name?") | execs the pinned binary; prints "Name?"; `reproduced` |
| 12 | replay with a build that has **no ledger code** | **runs live**: reads real stdin ("Bob", "99"), no ledger involved, exit 0 |
| 13 | `roc build` over the app after recording | pinned clone untouched; replay reproduces |
| 14 | Ctrl-C during a slowed replay | `interrupted after seq 3`, exit 130; supervisor and child in one pgrp |
| 14b | `kill -INT` and `kill -QUIT` sent to the replay child only | ignored (blocked); replay finishes `reproduced` |
| 15 | Roc `crash` after the name | `crashed: asked to crash`; root span status `ERROR`; replay reproduces |

Not exercised: a kill between crossings (`RESPOND.die_after`), a Rust panic, an `expect` failure.

Sample ledger (Ctrl-C), decoded:

```
seq= 1 hosted_stdout_line  parent=4dfd65  req=b'Name?'  resp=b'\x00'
seq= 2 hosted_stdin_line   parent=4dfd65  req=b''       resp=b'\x00Ann'
seq= 3 hosted_stdout_line  parent=4dfd65  req=b'Age?'   resp=b'\x00'
seq= 0 main!               span=4dfd65    req=<argv>    resp={"in_flight":{"effect":"hosted_stdin_line","request":"","seq":4},"signal":2}
                           status={code: ERROR, message: "killed by SIGINT during seq 4 (hosted_stdin_line)"}
```

## Surprises

- **Ignored signals survive exec.** The recording supervisor ignores SIGINT, so it must reset SIGINT, SIGQUIT and SIGHUP to `SIG_DFL` in `pre_exec`. Without that, the app can't be Ctrl-C'd while recording, which changes the behavior being recorded.
- **The frame fd must go back to close-on-exec in the child**, or a grandchild started by `Cmd` keeps the pipe open, and the supervisor never sees EOF.
- **`RETURNED` is its own frame** because the exit code isn't `roc_main`'s return: `dbg`/`expect` force 1.
- **The child makes no spans.** The supervisor timestamps BEGIN/END when they arrive and writes OTLP JSON itself with `serde_json`, so the child needs no OTEL code at all.
- **argv0 is preserved**, so `ps` shows `app/two-questions` for both the supervisor and the child, even though the child runs `<dir>/exe`.
- **The replay child still writes its own stderr**: `[ROC CRASHED]` comes from the glue's default handler. Echo from the CRASHED frame instead, or keep stderr inherited so `dbg` and panics show up?
- **Small race in the prototype**: on Ctrl-C mid-replay the supervisor finished its current echo ("Age?") before reporting. It should check the interrupt flag before echoing.
- **Size**: the app grows from 403 KB to 648 KB with this code compiled in (this includes `serde_json`, `base64`, and `std::process`).
- **Gap**: a recording child blocked in `read` never notices if its supervisor dies (macOS has no `PDEATHSIG`). It notices only at its next frame write.
