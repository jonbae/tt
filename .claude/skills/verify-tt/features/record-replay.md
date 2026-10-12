# Record and replay

A developer records a request with `ROC_LEDGER=record:<dir>`, then replays that ledger later, somewhere else, with none of the world it ran in. The replay prints what the request printed and ends the way it ended, even when the failure needed one particular file at one particular minute.

## Sub-features

- `record-crash` records a request that crashed, leaving a ledger with its pinned binary.
- `control` reruns the same request live after the world changed, and it no longer crashes.
- `replay-elsewhere` replays only the ledger directory, under `env -i` and with no input files, and reproduces the crash.
- `record-clean` records and replays a request that returned 0.
- `replay-pinned` replays through any ledger-aware binary, which execs the ledger's own `exe`.
- `always-on` records every request under a new name with `ledgered`, sharing one pin per build.

## How to get to it (user POV)

- Run `ROC_LEDGER=record:<dir> ./app args…` to record one request.
- Run `ROC_LEDGER=replay:<dir> <dir>/exe`, or the same thing through any ledger-aware app, to replay one.
- Source `$PROTO/ledger-on.sh` and put `ledgered` before the app to record every request into `${ROC_LEDGER_DIR:-~/.ledgers}`.

## Driving it with tx

Preconditions:

- `PROOF=$("$S/new-proof.sh" record-replay)`, and `"$S/doctor.sh" "$PROOF"` is all ok.
- Each call starts with `. <proof dir>/env.sh && cd "$PROOF/user"`.

- **Set up the moment.** Run `due=$("$S/agenda-clock.sh" due-now tasks.txt) && echo "due $due" &&` followed by the record command below, all in one shell call, because the crash only happens inside this minute. It waits for UTC second 2–50, then writes `tasks.txt` with `Submit essay draft` due this minute.
- **Record the crash.** Run `tx env ROC_LEDGER=record:ledgers/crash.ledger ./agenda tasks.txt`. The output is `Agenda at <now>`, `00:01  Early thing  done`, `[ROC CRASHED] Integer subtraction overflowed`, `[ledger] recorded crashed: Integer subtraction overflowed (shared) -> ledgers/crash.ledger`, then `exit 1`. `ledgers/crash.ledger/` holds `spans.jsonl` and `exe`.
- **Control.** Run `"$S/agenda-clock.sh" after tasks.txt && tx ./agenda tasks.txt`. All three items print, `Submit essay draft` reads `done`, and the run ends `exit 0`. The same file a minute later doesn't crash.
- **Take the world away.** Run `cp -R ledgers/crash.ledger "$PROOF/elsewhere/" && cd "$PROOF/elsewhere" && tx ls -A`. The only entry listed is `crash.ledger`, with no `tasks.txt` and no app.
- **Replay elsewhere.** Run `tx env -i ROC_LEDGER=replay:crash.ledger crash.ledger/exe`. It prints the recorded `Agenda at <now>` timestamp (not the current time), `00:01  Early thing  done` and `[ROC CRASHED] Integer subtraction overflowed`, then `[ledger] reproduced: crashed: Integer subtraction overflowed after 4 crossings`, then `exit 0`.
- **Record clean.** Back in `user/`, run `printf '00:01 Early thing\n23:59 Late thing\n' > fine.txt` and `tx env ROC_LEDGER=record:ledgers/fine.ledger ./agenda fine.txt`. That ends with `[ledger] recorded returned 0 (shared) -> ledgers/fine.ledger` and `exit 0`. Copy the ledger to `elsewhere/` and replay it as above, which ends with `[ledger] reproduced: returned 0 after 5 crossings`.
- **Replay pinned.** In `user/`, run `tx env ROC_LEDGER=replay:ledgers/fine.ledger ./two-questions`. The agenda output appears with `reproduced: returned 0 after 5 crossings`, because replay execs the ledger's pinned `agenda`, not `two-questions`.
- **Always on.** Run `tx env ROC_LEDGER_DIR="$PROOF/user/.ledgers" bash -c '. "$1"; ledgered ./agenda fine.txt; ledgered ./agenda fine.txt' _ "$PROTO/ledger-on.sh"`. Two `[ledger] recorded returned 0 (shared) -> …/.ledgers/agenda-<UTC>-<pid>-<random>.ledger` lines print. Then run `tx stat -f '%l %N' .ledgers/*.ledger/exe .ledgers/.pins/*`, which shows link count 3 on all three paths: one pin, hardlinked by both ledgers.
- **Proof.** The transcript holds the crash recording, the control's different output, `ls -A` showing `elsewhere/` holds only the ledger, and the `reproduced` verdict with the recorded timestamp.

## Gotchas

- If the recording doesn't crash, the minute rolled over between `due-now` and the record. Record again under a new ledger name, since an existing name exits 73.
- A live rerun prints a new timestamp, so compare the whole output by eye, not with a diff.
- The first crossing lands about 290 ms in when the pin is new. That's macOS's cost for a first exec of a new file, not a regression, and it shows in `dump`'s `at` column.
- An app built on a host without the ledger switch ignores `ROC_LEDGER`, runs live and leaves no ledger. Check that the ledger directory exists after every record, and run the doctor if it doesn't.
- Only five hosted functions have wrappers: `hosted_stdout_line`, `hosted_stderr_line`, `hosted_stdin_line`, `hosted_file_read_utf8` and `hosted_utc_now`. During replay, any other effect does its real work in the world and goes undetected. A demo app changed to use another effect quietly breaks the take-the-world-away guarantee. That includes close cousins like `Stdout.write!` and `Stderr.write!` (`hosted_stdout_write`, `hosted_stderr_write`), as well as `Env`, `File.write` and `Cmd`.
- `cp -R` copies `exe` as a plain file, which breaks its hardlink to the pin. Replay doesn't care.
