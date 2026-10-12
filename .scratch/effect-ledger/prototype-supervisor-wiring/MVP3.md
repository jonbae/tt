# MVP 3: observability from the ledger (throwaway)

Tests the map's fourth use: from the ledger alone you can see what a request did, and because each entry is a standard OTEL span, a normal trace viewer shows it too. Built on [MVP 1](MVP.md), on the same branch (`prototype/ledger-supervisor-wiring`, uncommitted), with the same Roc nightly on macOS x86_64.

## What's added

| Path | What |
|---|---|
| `basic-cli/src/ledger_prototype.rs` | `ROC_LEDGER=dump:<dir>`: like replay, it first execs `<dir>/exe`, then prints every crossing and the outcome. `become_pinned` is factored out of replay |
| `basic-cli/src/ledger_codec.rs` | one display fn per wrapped symbol, in the terms of its `Host.roc` signature; `iso_utc`; 2 more tests (7 in all) |
| `basic-cli/src/lib.rs` | `decode_stdin_line_response`, pulled out of the wrapper so replay and dump share it |
| `otlp-send.sh` | POSTs each `spans.jsonl` line, unchanged, to OTLP/HTTP `:4318/v1/traces` |
| `mvp3-test.sh` | the whole test |
| `mvp3/` | the ledgers from the first hand runs, plus a damaged copy |

## Run

```sh
docker run -d --name ledger-jaeger -p 16686:16686 -p 4318:4318 jaegertracing/jaeger:latest
cd basic-cli && cargo build --release --lib && cp target/release/libhost.a platform/targets/x64mac/
cargo test --release --lib ledger_codec
cd ../app && R=../roc-nightly/roc_nightly-macos_x86_64-2026-09-26-d6267b4/roc
$R build agenda.roc --output=agenda && $R build two_questions.roc --output=two-questions
cd .. && ./mvp3-test.sh          # then open http://localhost:16686
```

## Result (verbatim excerpt, 2026-10-08 UTC)

The dumps run in a clean directory with `env -i`, with only the ledger directories and no app files.

```
ledger   crash.ledger
request  4dc66c7f2eb05a68247268d2b2b93049
service  agenda (basic-cli 0.23.0, pinned shared)
args     ["./agenda", "tasks.txt"]
started  2026-10-08T00:41:34.489769Z

 seq         at  crossing
   1  +299.61ms  hosted_file_read_utf8(UnixBytes("tasks.txt")) -> Ok("09:00 Standup\n00:41 Submit essay draft\n23:59 Call home\n")
   2  +299.74ms  hosted_utc_now() -> Ok(1791420094789375000)  # 2026-10-08T00:41:34.789375Z
   3  +299.78ms  hosted_stdout_line("Agenda at 2026-10-08T00:41:34Z") -> Ok({})
   4  +299.81ms  hosted_stdout_line("09:00  Standup  in 498 min") -> Ok({})
 end  +299.88ms  crashed: Integer subtraction overflowed

ledger   ctrlc.ledger
request  006f5ebea03b760537aa9c3e4f1631ac
service  two-questions (basic-cli 0.23.0, pinned shared)
args     ["./two-questions"]
started  2026-10-08T00:41:35.133349Z

 seq         at  crossing
   1  +290.88ms  hosted_stdout_line("Name?") -> Ok({})
   2  +291.10ms  hosted_stdin_line() -> Ok("Ann")
   3  +291.26ms  hosted_stdout_line("Age?") -> Ok({})
   4             hosted_stdin_line() -> no response: SIGINT
 end  +291.49ms  killed by SIGINT during seq 4 (hosted_stdin_line)

== 3. OTLP: Jaeger at localhost:4318
crash.ledger: sent 5 lines, 0 rejected
fine.ledger: sent 6 lines, 0 rejected
ctrlc.ledger: sent 4 lines, 0 rejected
crash  traces 1, spans 5, roots 1, error=True, status='crashed: Integer subtraction overflowed'
fine   traces 1, spans 6, roots 1, error=False, status='ok'
ctrlc  traces 1, spans 4, roots 1, error=True, status='killed by SIGINT during seq 4 (hosted_stdin_line)'
```

- **The dump answers the test questions.** Without `agenda.roc`, you can see the request read `tasks.txt`, which held an item due in the current minute (`00:41`). It got the time `00:41:34`, printed two lines, and crashed on the next item, which is the one due that minute. The dump stops at crossings: it can't tell which Roc expression crashed between seq 4 and the end. That needs time-travel debugging (use 2).
- **Jaeger takes the lines unchanged**: one trace per request, one root, and the crashed or killed root marked `error=true` with a readable status message. These were checked through Jaeger's query API, not by eye.
- **A damaged ledger** (root line cut in half, one payload not base64) dumps whatever can be read, then lists `line 2: not a ledger span` and `line 5: torn`. Jaeger rejects the torn line with HTTP 400 and accepts the rest.
- Codec tests: 7 passed.

## Findings

- **Base64 hides everything except the shape.** In Jaeger, the symbol names, sequence numbers, timings and the root's status are readable. Every request and response is base64 (`ledger.request = QWdlbmRhIGF0…`). So a trace viewer shows *that* the request read a file, got the time, printed twice and crashed, but not *which* file, what was in it, what time it got, or what it printed. That's an input to spec §5: readable payloads in a stock viewer need readable attribute values. Two options are text-valued payloads stored as plain strings, or a readable rendering written next to the exact bytes.
- **`dump` can't read ledgers recorded before it existed.** The ledger's pinned binary does the decoding, so the reader is fixed when the request is recorded. `ROC_LEDGER=dump:<MVP 1 ledger> ./agenda` execs the MVP 1 binary, which answers `ROC_LEDGER must be record:<dir> or replay:<dir>` and exits 64. The same ledger's spans load into Jaeger, because the OTLP fields don't depend on the build. This is the map's rule, that the CLI reads standard OTLP fields only and delegates the rest to each ledger's pinned binary, showing its cost: a new or fixed `dump` only helps ledgers recorded after it.
- **The `at` column made MVP 1's first-exec cost visible with no extra tooling.** The first crossing comes at about +300 ms when the shared pin is new (a fresh `.pins/` per test run) and about +5 ms when it already exists.
- **Copies of a ledger share trace and span IDs.** Sending an edited copy merges it into the original's trace, and Jaeger warns `duplicate span IDs; skipping clock skew adjustment`.
- **Size:** the five display fns plus `iso_utc` take about 1.5 KB of code, and the dump driver about 9 KB. These figures come from gaps between symbol addresses, since macOS `nm` reports no sizes, and inlining blurs them. Across the whole binary, `__text` grew 45 KB (484 → 530 KB, +9%), and I haven't worked out where the rest goes. The map asks for the size of a codec table keyed by symbol: about 300 B per symbol here, which would be roughly 25 KB for all 85, if the hand-written ones are typical.
- Dump and Jaeger both use the hosted symbol (`hosted_file_read_utf8`), not the Roc call the developer wrote (`Path.read_utf8!`). That was readable enough for these five symbols.

## Not tested

- Large payloads. `dump` never truncates, so a big file would fill the screen. OTLP's 4 MiB request limit wasn't reached either.
- Handles, codegen, Linux, the Jaeger UI by eye, any collector other than Jaeger.
