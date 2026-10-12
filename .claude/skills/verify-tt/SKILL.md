---
name: verify-tt
description: Prove effect-ledger (tt) behavior by driving the real prototype. Build the ledger-enabled basic-cli host, record Roc requests with ROC_LEDGER, take the world away, replay and dump the ledgers, and keep transcripts and ledgers as evidence. Use after changing the ledger host or the demo apps, or to demo, reproduce or verify recording, replay, refusal, dump or OTLP export.
---

# Verify tt

tt's surface is one environment variable on a Roc app built against the ledger host: `ROC_LEDGER=record:<dir>`, `replay:<dir>` or `dump:<dir>`. A ledger is a directory holding `spans.jsonl` (one OTLP span per crossing, with the `main!` root last) and `exe` (the pinned binary). Speak in the terms in `GLOSSARY.md` at the repo root: request, crossing, outcome, replay, divergence.

The code isn't in tt's git history. It's the uncommitted basic-cli branch `prototype/ledger-supervisor-wiring` under the gitignored `.scratch/effect-ledger/prototype-supervisor-wiring/` (`$PROTO` below), where `README.md`, `MVP.md` and `MVP3.md` say what was built. A checkout without `.scratch/` can't run this skill.

## What a proof is

tt is a record and replay tool in rr's mould. A Roc request is a deterministic function of its effect responses. Roc has no tracing GC (it uses reference counting with opportunistic in-place mutation), so only a crossing can change what the core does. That gives the method:

1. **Record** the request against the real world.
2. **Take the world away.** Copy only the `.ledger` directory into `$PROOF/elsewhere/`, and replay it there under `env -i` with no input files. Replay already gives the child `/dev/null` for stdin.
3. **Replay.** The app prints what it printed when it was recorded, and the ledger says `[ledger] reproduced: <outcome> after N crossings` and exits 0.
4. **Control.** Show that the world alone wouldn't give that result. Rerun live after the world changed (the minute passed, different stdin) and capture the different output. Without the control, a reproduction can't be told apart from a request that always does the same thing.

`reproduced`, `divergence` (exit 70) and `refused` (exit 65) are the ledger's verdicts on itself: whether the pinned binary replayed its own recording. They say nothing about the app. Whether the Roc app behaved correctly is the developer's call, so a proof shows what happened and asserts only tt's own contract.

Proof standards:

- **Drive the real path.** Use the `ROC_LEDGER` switch on a built app, and give interactive apps real stdin through a pty. `$PROTO/drive.py` sends Ctrl-C as byte `0x03`, so the line discipline signals the whole process group the way a terminal does.
- **Capture both sides.** That means the recording's output and its `[ledger] recorded …` line, the replay's output and verdict, and the exit status the shell saw.
- **Keep the ledgers.** They're the side effect. A transcript that says `recorded` with no ledger directory beside it proves nothing.
- **Edit ledgers only to damage them.** Hand-edited ledgers are inputs only for `damaged-ledgers` proofs. Make them with `damage.py` from a real recording, which copies first and leaves the original intact.

## Launch

No server runs, because every request is a short-lived process. Launch means a current host plus a proof dir with its own app builds.

```sh
S=<tt repo root>/.claude/skills/verify-tt/scripts
"$S/doctor.sh"                     # all ok?
"$S/build.sh"                      # only if the doctor says the host is stale, or you changed basic-cli/src
PROOF=$("$S/new-proof.sh" <feature-id>)
"$S/doctor.sh" "$PROOF"            # ready when every line is ok
```

`build.sh` rebuilds the host, installs it into `basic-cli/platform/targets/x64mac/libhost.a`, runs the codec tests and reruns the doctor. That `libhost.a` is the one piece of shared state, so run `build.sh` only while no other agent is mid-proof. Everything else is per proof dir, so concurrent proofs are safe.

`new-proof.sh` builds `agenda` and `two-questions` into `$PROOF/user/` from `$PROTO/app/*.roc` in about 3 s. It records the host hash, the uncommitted paths and the app hashes in `$PROOF/build.txt`, and writes `$PROOF/env.sh`.

Shell state doesn't persist between tool calls, so start every call with `. "$PROOF/env.sh"`, using the literal path. It sets `PROOF`, `ROOT` (the tt repo), `S` and `PROTO`, and defines `tx`.

## Doctor

`"$S/doctor.sh" ["$PROOF"]` is read-only. It checks macOS x86_64, the tools, the Roc nightly version, the basic-cli branch, that the host is newer than its sources and carries the `ROC_LEDGER` switch, and whether the Docker daemon is up. Given a proof dir, it also checks that both apps were built after the host and answer `ROC_LEDGER=doctor:x` with exit 64, and lists processes still running inside the proof. Run it first whenever something looks off: a recording that left no ledger directory, an app that ignored `ROC_LEDGER`, or an unexpected exit 64.

## Drive

`tx <command…>` runs a command from inside the proof dir, shows its output, and appends the command, its combined stdout and stderr, and its exit to `$PROOF/transcript.txt`. Spell environment variables with `env`, so they land in the transcript.

```sh
. "$PROOF/env.sh" && cd "$PROOF/user"
tx env ROC_LEDGER=record:ledgers/fine.ledger ./agenda fine.txt
cp -R ledgers/fine.ledger "$PROOF/elsewhere/" && cd "$PROOF/elsewhere"
tx env -i ROC_LEDGER=replay:fine.ledger fine.ledger/exe
tx env -i ROC_LEDGER=dump:fine.ledger fine.ledger/exe
cd "$PROOF/user"
tx python3 "$PROTO/drive.py" "wait:Name?,send:Ann,wait:Age?,ctrlc" -- env ROC_LEDGER=record:ledgers/ctrlc.ledger ./two-questions
```

- **`drive.py` steps:** `wait:<text>`, `send:<line>`, `ctrlc`, `eof`, `sigterm` (to the recording supervisor), `sleep:<secs>`. It exits 0 itself, and the app's end is its last line, `<< shell sees: exit N >>` or `<< shell sees: killed by SIGINT >>`.
- **Replay knobs:** `ROC_LEDGER_MARK=1` prefixes each echoed line with `[seq N]`. `ROC_LEDGER_SLOW_MS=<ms>` pauses before each crossing, so a replay can be interrupted midway.
- **Exit codes:** a recording exits the way the app did, re-raising its signal. Replay and dump exit as follows.

  | Exit | Meaning |
  |---|---|
  | 0 | reproduced (replay), or dumped |
  | 64 | `ROC_LEDGER` isn't `record:`, `replay:` or `dump:` |
  | 65 | refused: a damaged or incomplete ledger |
  | 66 | dump with no `exe` or no `spans.jsonl` |
  | 70 | divergence |
  | 71 | the pinned binary couldn't be started |
  | 73 | the record target already exists |
  | 130 | replay interrupted |

The feature map in `features/` holds the recipes, one file per feature. Read `features/README.md` before driving.

## Evidence

All evidence lives in the proof dir, `.scratch/verify-tt/proofs/<UTC>-<feature-id>-<pid>/`, which is gitignored with the rest of `.scratch/`:

- **`transcript.txt`:** every `tx` command with its output and exit.
- **`build.txt`:** the exact host and app builds that were proved.
- **`user/`:** the world, meaning input files like `tasks.txt`, plus the ledgers recorded there and their shared pins in `user/ledgers/.pins/`.
- **`elsewhere/`:** the ledger copies that were replayed or dumped with the world taken away.
- **`*.jaeger.json`:** Jaeger's own view of each sent trace, for `otlp-jaeger`.

Report a proof as the proof dir's path plus the transcript lines that show the action and its result, quoted verbatim. When a step can't run, report the command you tried and the unmet precondition, and mark that sub-feature unverified.

## Cleanup

Run `"$S/cleanup.sh" "$PROOF"` after every proof, failed attempts included. It stops the processes running a binary from inside this proof dir (its app builds and its ledgers' pinned `exe`s), which are exactly the ones this proof started. It never matches by process name, because every proof's apps share the names `agenda` and `two-questions`. It removes the Jaeger container only if this proof started it (`$PROOF/jaeger.container`), and deletes the proof's two app builds. The ledgers keep their pinned copies, and the evidence stays. The script then lists what it kept, and exits 1 if `transcript.txt` or `build.txt` is missing. Old proof dirs accumulate under `.scratch/verify-tt/proofs/`, and deleting them is the user's call.

## Helpers

Everything in `scripts/` is executable:

| Script | Use |
|---|---|
| `doctor.sh [proof]` | read-only health check |
| `build.sh` | rebuild and install the shared host, run the codec tests |
| `new-proof.sh <feature-id>` | make a proof dir, build both apps into it, print its path |
| `transcript.sh <cmd…>` | run and record a command (`tx` in `env.sh`) |
| `agenda-clock.sh due-now <file>` / `after <file>` | set up the agenda crash's minute, then wait it out |
| `damage.py <kind> <src.ledger> <dst.ledger>` | copy a ledger and damage the copy (`--help` lists the kinds) |
| `jaeger-up.sh <proof>` | start this proof's own Jaeger, wait until it answers, write its ports to `<proof>/jaeger.env` |
| `jaeger-traces.sh <host:port> <dir>.ledger…` | read each ledger's trace back from Jaeger, with retries, and save its JSON |
| `cleanup.sh <proof>` | stop this proof's processes, drop scratch, list the evidence kept |
