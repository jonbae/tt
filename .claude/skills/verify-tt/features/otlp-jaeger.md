# OTLP export to Jaeger

Every ledger entry is already an OTLP/JSON export request, so a developer can POST each `spans.jsonl` line, unchanged, to any OTLP/HTTP collector. In Jaeger, each request shows up as one trace with one root, and a crashed or killed request has its root marked as an error with a readable status. Payloads stay base64, so the viewer shows the request's shape: functions, sequence numbers, timings and outcome, but not its values.

## Sub-features

- `otlp-accept` sends every line of a ledger, and the collector accepts all of them.
- `trace-shape` gives each ledger one trace with one root and one span per crossing.
- `error-root` marks a crashed or killed root `error=true`, with the outcome as its status.
- `torn-rejected` has the collector reject a damaged ledger's torn line (HTTP 400) and accept the rest.

## How to get to it (user POV)

- Run `OTLP_ENDPOINT=http://<host:port> $PROTO/otlp-send.sh <dir>.ledger…`, then open Jaeger's UI.

## Driving it with tx

Preconditions:

- `"$S/doctor.sh"` prints `docker daemon up`. If it's down, report this feature unverified with that line, and ask the user before starting Docker Desktop (`open -a Docker`).
- `PROOF=$("$S/new-proof.sh" otlp-jaeger)`, plus `crash.ledger`, `fine.ledger` (record-replay) and `ctrlc.ledger` (interactive-and-signals) recorded in this proof and copied into `elsewhere/`.
- This proof starts its own Jaeger on random loopback ports, and leaves alone any `ledger-jaeger` container the user runs on 16686 and 4318.

- **Start Jaeger.** Run `tx "$S/jaeger-up.sh" "$PROOF"`. It prints `OTLP=127.0.0.1:<port>`, `UI=127.0.0.1:<port>` and `container verify-tt-<proof name>`, once Jaeger answers its query API. Start each later shell call with `. "$PROOF/env.sh" && . "$PROOF/jaeger.env"`.
- **Send.** In `elsewhere/`, run `tx env OTLP_ENDPOINT="http://$OTLP" "$PROTO/otlp-send.sh" crash.ledger fine.ledger ctrlc.ledger`. Each ledger prints `<dir>: sent N lines, 0 rejected`.
- **Read back.** Run `tx "$S/jaeger-traces.sh" "$UI" crash.ledger fine.ledger ctrlc.ledger`, which saves `<name>.jaeger.json` beside each ledger. Expect `crash` to show `traces 1, … roots 1, error=True, status='crashed: Integer subtraction overflowed'`, `fine` to show `error=False, status='ok'`, and `ctrlc` to show `error=True, status='killed by SIGINT during seq 4 (hosted_stdin_line)'`. Span counts equal the line counts of each `spans.jsonl`.
- **Torn line.** Damage a ledger that was never sent, so its spans can't merge into a trace Jaeger already holds. In `user/`, run `tx env ROC_LEDGER=record:ledgers/spare.ledger ./agenda fine.txt` and `cp -R ledgers/spare.ledger "$PROOF/elsewhere/"`. Then in `elsewhere/`, run `tx "$S/damage.py" torn spare.ledger torn.ledger` and `tx env OTLP_ENDPOINT="http://$OTLP" "$PROTO/otlp-send.sh" torn.ledger`. It prints `  line 6: HTTP 400` (the torn root), then `torn.ledger: sent 6 lines, 1 rejected`.
- **Proof.** Keep the `*.jaeger.json` files: they're Jaeger's own view, and they outlive the container. Cleanup removes the container.

## Gotchas

- Jaeger 2.22 (`jaegertracing/jaeger:latest` as of 2026-10) dropped the legacy `/api/services`, and serves `/api/v3/services` instead. The legacy `/api/traces/<id>` still answers, and it's what `jaeger-up.sh` polls and `jaeger-traces.sh` reads. If a newer image drops it too, both scripts need the v3 API.
- Copies of a ledger share trace and span IDs, so a ledger and its edited copy merge into one trace in the same Jaeger. That's why the torn step damages a spare ledger that was never sent.
- Base64 payloads in Jaeger are expected (`ledger.request = QWdlbmRh…`). Readable values come from `dump`.
- Seeing a trace by eye in the UI isn't proof. The JSON from `jaeger-traces.sh` is.
