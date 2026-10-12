# Dump

A developer runs `ROC_LEDGER=dump:<dir>` to see what a request did, from its ledger alone. The ledger's pinned binary decodes its own payloads and prints the request's header, then every crossing with its sequence number, time offset, hosted function, readable request and response, and finally the outcome. A damaged ledger dumps whatever is readable, then lists the lines it couldn't read.

## Sub-features

- `dump-crash` shows what a crashed request read, printed and how it ended, with no app source or input files present.
- `dump-interrupted` shows the in-flight crossing of a request killed by a signal.
- `dump-damaged` dumps the readable crossings of a damaged ledger and lists its bad lines.
- `dump-no-exe` refuses (66) when no pinned binary is there to decode, or when there's no `spans.jsonl`.
- `dump-old-ledger` shows that a ledger recorded before `dump` existed can't be dumped, because its reader was fixed when it was recorded.

## How to get to it (user POV)

- Run `ROC_LEDGER=dump:<dir> <dir>/exe`, or the same through any ledger-aware app. Either way it execs the ledger's own `exe`.

## Driving it with tx

Preconditions:

- `PROOF=$("$S/new-proof.sh" dump)`, and `"$S/doctor.sh" "$PROOF"` is all ok.
- `crash.ledger` (record-replay's `record-crash` step) and `ctrlc.ledger` (interactive-and-signals' `ctrlc-record` step) are recorded in this proof's `user/` and copied into `elsewhere/`.
- Each dump runs in `elsewhere/` under `env -i`.

- **Dump a crash.** Run `tx env -i ROC_LEDGER=dump:crash.ledger crash.ledger/exe`. The header shows `ledger crash.ledger`, `request <traceId>`, `service agenda (basic-cli 0.23.0, pinned shared)`, `args ["./agenda", "tasks.txt"]` and `started <UTC>`. The rows are `1 … hosted_file_read_utf8(UnixBytes("tasks.txt")) -> Ok("…")` with the file's contents, `2 … hosted_utc_now() -> Ok(<ns>)  # <ISO time>`, and two `hosted_stdout_line(…) -> Ok({})` rows, and the last line is `end … crashed: Integer subtraction overflowed`. It exits 0.
- **Dump an interrupt.** Run `tx env -i ROC_LEDGER=dump:ctrlc.ledger ctrlc.ledger/exe`. Row 4 reads `hosted_stdin_line() -> no response: SIGINT` with an empty `at` column, and the last line is `end … killed by SIGINT during seq 4 (hosted_stdin_line)`.
- **Dump a damaged ledger.** Run `tx "$S/damage.py" torn crash.ledger torn.ledger`, then `tx env -i ROC_LEDGER=dump:torn.ledger torn.ledger/exe`. The header loses its `args` and `started` lines, because those come from the root. The readable rows still print, then `end  no main! span: the request never finished, or its supervisor was killed`, then `[ledger] line 5: torn` (the root's line number). It exits 0.
- **No exe.** Run `tx "$S/damage.py" no-exe crash.ledger noexe.ledger`, then `tx env -i ROC_LEDGER=dump:noexe.ledger crash.ledger/exe`. It prints `[ledger] no pinned binary, so nothing here can decode the payloads` and exits 66.
- **No spans.** Run `tx "$S/damage.py" no-spans crash.ledger nospans.ledger`, then `tx env -i ROC_LEDGER=dump:nospans.ledger nospans.ledger/exe`. It prints `[ledger] no spans.jsonl` and exits 66.
- **Old ledger.** Run `cp -R "$ROOT/effect-ledger-demo/agenda-20261007T190745Z-3007-28676.ledger" mvp1.ledger`, then `tx env -i ROC_LEDGER=dump:mvp1.ledger mvp1.ledger/exe`. It prints `[ledger] ROC_LEDGER must be record:<dir> or replay:<dir>` and exits 64. `tx env -i ROC_LEDGER=replay:mvp1.ledger mvp1.ledger/exe` still ends `[ledger] reproduced: crashed: Integer subtraction overflowed after 4 crossings`.
- **Proof.** The transcript holds each dump verbatim. From the crash dump alone, a reader can tell which file was read and what it held, what time came back, what was printed, and that the request crashed after seq 4.

## Gotchas

- `dump` writes everything to stdout, its `[ledger] line N: …` problem list included, while replay writes its `[ledger]` lines to stderr. To split the streams, redirect to separate files (`>out 2>err`). zsh's MULTIOS copies stdout into a `2>&1 >/dev/null |` pipe, which makes stdout look like stderr.
- `dump` stops at crossings. It can't say which Roc expression crashed between the last row and `end`, and no recipe should claim it does.
- Ledgers recorded before MVP 3 can't be dumped, by design: the reader is the pinned binary. Exit 64 on such a ledger isn't a regression.
- `dump` never truncates, so a big file read floods the transcript. Keep demo inputs small.
- The first `at` value is about +290 ms on a fresh pin and about +5 ms on a reused one. That's macOS's first-exec cost, not a regression.
- `effect-ledger-demo/` belongs to the user. Copy from it and dump the copy.
