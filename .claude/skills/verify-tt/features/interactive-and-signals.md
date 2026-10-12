# Interactive requests and signals

A developer records an interactive request typed at a terminal, including one cut short by Ctrl-C, Ctrl-D or SIGTERM. The ledger records how it ended, with the crossing that was in flight. Replay needs no stdin, and it reproduces that ending. Replays themselves can be interrupted.

## Sub-features

- `stdin-happy` records typed answers and replays them with no stdin.
- `ctrlc-record` records a request killed by SIGINT while it waits on stdin, and replays it.
- `eof-record` records Ctrl-D at a prompt (the app returns 1) and replays it.
- `sigterm-record` records SIGTERM sent to the recording supervisor, which forwards it to the app.
- `replay-interrupt` makes Ctrl-C during a slowed replay end it with exit 130.
- `replay-mark` tags each echoed line with its sequence number.

## How to get to it (user POV)

- Run `ROC_LEDGER=record:<dir> ./two-questions` in a terminal and type, or press Ctrl-C or Ctrl-D.
- Run `kill -TERM <supervisor pid>` against a recording.
- Run `ROC_LEDGER=replay:<dir> <dir>/exe`, optionally with `ROC_LEDGER_SLOW_MS=<ms>` or `ROC_LEDGER_MARK=1`.

## Driving it with tx

Preconditions:

- `PROOF=$("$S/new-proof.sh" interactive-and-signals)`, and `"$S/doctor.sh" "$PROOF"` is all ok.
- Each call starts with `. <proof dir>/env.sh && cd "$PROOF/user"`. `D="$PROTO/drive.py"` is shorthand below.

- **Happy record.** Run `tx python3 "$D" "wait:Name?,send:Ann,wait:Age?,send:30" -- env ROC_LEDGER=record:ledgers/happy.ledger ./two-questions`. The output shows `Hi Ann, 30 is a fine age.`, then `[ledger] recorded returned 0 (shared) -> ledgers/happy.ledger`, then `<< shell sees: exit 0 >>`.
- **Control.** Run `tx python3 "$D" "wait:Name?,send:Bob,wait:Age?,send:99" -- ./two-questions`, which shows `Hi Bob, 99 is a fine age.`
- **Happy replay.** Copy `happy.ledger` into `elsewhere/` and run `tx env -i ROC_LEDGER=replay:happy.ledger happy.ledger/exe` there. It prints `Name?`, `Age?` and `Hi Ann, 30 is a fine age.`, then `[ledger] reproduced: returned 0 after 5 crossings`, then `exit 0`.
- **Ctrl-C.** Run `tx python3 "$D" "wait:Name?,send:Ann,wait:Age?,ctrlc" -- env ROC_LEDGER=record:ledgers/ctrlc.ledger ./two-questions`, which shows `[ledger] recorded killed by SIGINT during seq 4 (hosted_stdin_line) (shared) -> ledgers/ctrlc.ledger` and `<< shell sees: killed by SIGINT >>`. Replayed in `elsewhere/`, it prints `Name?` and `Age?`, then `[ledger] reproduced: killed by SIGINT during seq 4 (hosted_stdin_line) after 3 crossings`, then `exit 0`.
- **Ctrl-D.** Use the steps `wait:Name?,send:Ann,wait:Age?,eof` into `ledgers/eof.ledger`. The output shows `Program exited with error: MissingAge`, `[ledger] recorded returned 1 …` and `<< shell sees: exit 1 >>`. The replay ends `[ledger] reproduced: returned 1 after 5 crossings`.
- **SIGTERM.** Use the steps `wait:Name?,send:Ann,wait:Age?,sigterm` into `ledgers/term.ledger`. The output shows `[ledger] recorded killed by SIGTERM during seq 4 (hosted_stdin_line) …` and `<< shell sees: killed by SIGTERM >>`. The replay reproduces it.
- **Interrupt a replay.** In `elsewhere/`, run `tx python3 "$D" "wait:Name?,ctrlc" -- env ROC_LEDGER_SLOW_MS=500 ROC_LEDGER=replay:happy.ledger happy.ledger/exe`. That shows `[ledger] interrupted after seq N` and `<< shell sees: exit 130 >>`.
- **Mark.** Run `tx env -i ROC_LEDGER_MARK=1 ROC_LEDGER=replay:happy.ledger happy.ledger/exe`, which prints `[seq 1] Name?`, `[seq 3] Age?` and `[seq 5] Hi Ann, 30 is a fine age.`
- **Proof.** For each ending, the transcript holds the recorded `<< shell sees >>` line, the matching `reproduced` verdict from `elsewhere/`, and the ledger directory.

## Gotchas

- `drive.py` itself always exits 0, so `tx` records `exit 0`. The app's real end is the `<< shell sees: … >>` line.
- `wait:` gives up silently after 10 s and the script carries on. If a prompt is missing from the output, the later steps ran against the wrong state, so redo the proof.
- Replay doesn't echo the typed answers (`Ann`, `30`). The terminal echoed them, and they never crossed the boundary. The answers live in the `hosted_stdin_line` responses, which `dump` shows.
- In the prototype, Ctrl-C mid-replay can finish echoing the current line before it reports. A stray line after `^C` is that known race.
- The prototype never exercised a kill between crossings, a Rust panic or an `expect` failure, and this map has no recipe for them yet.
