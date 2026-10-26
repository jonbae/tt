# Record and replay

A developer records a request with `ROC_LEDGER=record:<dir>`, then replays that ledger later, somewhere else, with none of the world it ran in. The replay prints what the request printed and ends the way it ended, even when the failure needed one particular file at one particular minute. A server records every request it handles into one ledger, and any one of them replays alone.

## Sub-features

- `record-crash` records a request that crashed, leaving a ledger with one instance and its pinned binary.
- `control` reruns the same request live after the world changed, and it no longer crashes.
- `replay-elsewhere` replays only the ledger directory, under `env -i` and with no input files, and reproduces the crash.
- `record-clean` records and replays a request that returned 0.
- `replay-pinned` replays through any ledger-aware binary, which hands each instance to that instance's own pinned `exe`.
- `add-instance` records into an existing ledger, which adds a second instance that shares the first one's pin.
- `server-request` records a server's requests into one instance, dumps them, and replays one request alone.

## How to get to it (user POV)

- Run `ROC_LEDGER=record:<dir> ./app args…` to record. Run it again with the same `<dir>` to add an instance.
- Run `ROC_LEDGER=replay:<dir> <dir>/<instance>/exe`, or the same thing through any ledger-aware app, to replay every instance.
- Add `ROC_LEDGER_REQUEST=<request id prefix>` to replay or dump one request.

## Driving it with tx

Preconditions:

- `PROOF=$("$S/new-proof.sh" record-replay)`, and `"$S/doctor.sh" "$PROOF"` is all ok.
- Each call starts with `. <proof dir>/env.sh && cd "$PROOF/user"`.

- **Set up the moment.** Run `due=$("$S/agenda-clock.sh" due-now tasks.txt) && echo "due $due" &&` followed by the record command below, all in one shell call, because the crash only happens inside this minute. It waits for UTC second 2–50, then writes `tasks.txt` with `Submit essay draft` due this minute.
- **Record the crash.** Run `tx env ROC_LEDGER=record:ledgers/crash.ledger ./agenda tasks.txt`. The output is `Agenda at <now>`, `00:01  Early thing  done`, `[ROC CRASHED] Integer subtraction overflowed`, `[ledger] recorded 1 request(s) (shared) -> ledgers/crash.ledger, instance <UTC>-<4 hex>`, `[ledger]   main! <request>: crashed: Integer subtraction overflowed`, then `exit 1`. `ledgers/crash.ledger/` holds `.pins/` and one instance directory with `exe`, `instance.json` and `spans.jsonl`.
- **Control.** Run `"$S/agenda-clock.sh" after tasks.txt && tx ./agenda tasks.txt`. All three items print, `Submit essay draft` reads `done`, and the run ends `exit 0`. The same file a minute later doesn't crash.
- **Take the world away.** Run `cp -R ledgers/crash.ledger "$PROOF/elsewhere/" && cd "$PROOF/elsewhere" && tx ls -A`. The only entry listed is `crash.ledger`, with no `tasks.txt` and no app.
- **Replay elsewhere.** Run `tx env -i ROC_LEDGER=replay:crash.ledger crash.ledger/*/exe`. It prints the recorded `Agenda at <now>` timestamp (not the current time), `00:01  Early thing  done` and `[ROC CRASHED] Integer subtraction overflowed`, then `[ledger] reproduced main! <request>: crashed: Integer subtraction overflowed after 4 crossings`, then `exit 0`.
- **Record clean.** Back in `user/`, run `printf '00:01 Early thing\n23:59 Late thing\n' > fine.txt` and `tx env ROC_LEDGER=record:ledgers/fine.ledger ./agenda fine.txt`. That ends with `[ledger]   main! <request>: returned 0` and `exit 0`.
- **Replay pinned.** In `user/`, run `tx env ROC_LEDGER=replay:ledgers/fine.ledger ./two-questions`. The agenda output appears with `[ledger] reproduced main! <request>: returned 0 after 5 crossings`, because replay runs the instance's pinned `agenda`, not `two-questions`.
- **Add an instance.** Run `tx env ROC_LEDGER=record:ledgers/fine.ledger ./agenda fine.txt` again. It prints `[ledger] recorded 1 request(s) (shared) -> ledgers/fine.ledger, instance <new id>`. `tx ls -A ledgers/fine.ledger` lists `.pins` and two instance directories. `tx sh -c 'stat -f "%l %N" ledgers/fine.ledger/*/exe ledgers/fine.ledger/.pins/*'` shows link count 3 on all three paths: one pin, hardlinked by both instances. Copy the ledger to `elsewhere/` and replay it through either instance's `exe`. Each instance prints `[ledger] instance <id>`, its own `Agenda at` timestamp and its own `reproduced main! <request>: returned 0 after 5 crossings`, and the replay exits 0.
- **Server request.** In `user/`, run `cp fine.txt tasks.txt`, then start `env ROC_LEDGER=record:ledgers/web.ledger ./agenda-server >server.log 2>&1 &` and wait for `Listening` in `server.log`. Run `tx curl -s -w ' [%{http_code}]\n' http://127.0.0.1:8137/agenda` (the agenda, `[200]`) and the same for `/nope` (`not found [404]`). Then `kill -TERM` the server and append `server.log` to the transcript. It ends `[ledger] recorded 4 request(s) (shared) -> ledgers/web.ledger, instance <id>`.
  - Copy the ledger to `elsewhere/` and run `tx env -i ROC_LEDGER=dump:web.ledger web.ledger/*/exe`. The timeline shows `start`, `init!()`, `respond!(GET /agenda, 3 headers)` with its `hosted_file_read_utf8`, `hosted_unix_time_now` and `hosted_stdout_line` rows, `respond!(GET /nope, 3 headers)` with none, `shutdown!(Terminate)`, then `exit  process exited 143`.
  - Run `tx env -i ROC_LEDGER=replay:web.ledger ROC_LEDGER_REQUEST=<the /agenda request> web.ledger/*/exe`. It prints `GET /agenda at <recorded seconds>s`, then `[ledger] reproduced respond! <request>: returned 200 [Content-Type: text/plain; charset=utf-8] "…" after 3 crossings`, then `exit 0`.
- **Proof.** The transcript holds the crash recording, the control's different output, `ls -A` showing `elsewhere/` holds only the ledger, the `reproduced` verdict with the recorded timestamp, and the server's dump and one-request replay.

## Gotchas

- If the recording doesn't crash, the minute rolled over between `due-now` and the record. Record again under a new ledger name. Recording into the same name adds an instance next to the one that didn't crash.
- A live rerun prints a new timestamp, so compare the whole output by eye, not with a diff.
- The first crossing lands about 290 ms in when the pin is new. That's macOS's cost for a first exec of a new file, not a regression, and it shows in `dump`'s time column.
- An app built on a host without the ledger switch ignores `ROC_LEDGER`, runs live and leaves no ledger. Check that the ledger directory exists after every record, and run the doctor if it doesn't.
- Replaying one server request re-runs its `init!` first, without echoing `init!`'s output, so only the request's own lines print.
- `prove.sh` also uses port 8137. Don't run it while a proof's server is up.
- Only some hosted functions have wrappers. Each fork's `BOUNDARY` in `src/ledger.rs` lists them. During replay, any other effect does its real work in the world and goes undetected, so a demo app changed to use another effect quietly breaks the take-the-world-away guarantee.
- `cp -R` copies `exe` as a plain file, which breaks its hardlink to the pin. Replay doesn't care.
