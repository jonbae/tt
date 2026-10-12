# Damaged ledgers

A developer replays a ledger that was cut short, truncated or edited. Replay refuses to start (exit 65) and names what's wrong, or it stops at the first divergence (exit 70). A divergence is a Mismatch, an Extra crossing the ledger lacks, a Missing crossing the replay never made, or a different outcome. Neither path ever runs the app against a ledger it can't trust. Recording refuses to overwrite an existing ledger.

## Sub-features

- `refuse-torn` refuses a ledger whose root line is cut in half.
- `refuse-no-root` refuses a ledger with no `main!` span.
- `refuse-gap` refuses a ledger with a missing sequence number.
- `refuse-alien` refuses a ledger that names a hosted function outside the boundary.
- `refuse-no-seq` refuses a line that is valid JSON but has no `ledger.seq`.
- `refuse-bad-b64` refuses a crossing whose request isn't base64.
- `refuse-bad-args` refuses a root whose request doesn't decode as process args.
- `refuse-bad-outcome` refuses a root whose response is JSON but not an outcome.
- `refuse-no-exe` refuses a ledger with no pinned binary.
- `refuse-no-spans` refuses a ledger with no `spans.jsonl`.
- `divergence-mismatch` stops a replay at a crossing whose recorded request differs from the replayed one.
- `divergence-extra` stops a replay at a crossing the ledger doesn't have.
- `divergence-missing` reports a recorded crossing the replay never made.
- `divergence-outcome` reports a replay that ends differently from the recorded outcome (`Mismatch at seq 0 (main!)`).
- `record-exists` refuses to record into an existing ledger directory.
- `bad-switch` rejects a `ROC_LEDGER` value that isn't `record:`, `replay:` or `dump:`.

## How to get to it (user POV)

- Run `ROC_LEDGER=replay:<damaged dir> <any ledger-aware app>`.
- Run `ROC_LEDGER=record:<existing dir> ./app`.
- Set `ROC_LEDGER` to anything malformed.

## Driving it with tx

Preconditions:

- `PROOF=$("$S/new-proof.sh" damaged-ledgers)`, and `"$S/doctor.sh" "$PROOF"` is all ok.
- A real recording exists to damage. In `user/`, run `tx python3 "$PROTO/drive.py" "wait:Name?,send:Ann,wait:Age?,send:30" -- env ROC_LEDGER=record:ledgers/happy.ledger ./two-questions`, then `cp -R ledgers/happy.ledger "$PROOF/elsewhere/"`.
- Each damage step runs in `elsewhere/`. Replay every damaged copy through `happy.ledger/exe`, so `no-exe` takes the same path as the rest.

- **Damage and replay.** For each `<kind>` in the table, run `tx "$S/damage.py" <kind> happy.ledger <kind>.ledger`, then `tx env -i ROC_LEDGER=replay:<kind>.ledger happy.ledger/exe`. Each prints the line shown and exits with the code shown.

  | Kind | Verdict line | Exit |
  |---|---|---|
  | `torn` | `[ledger] refused: torn line 6` | 65 |
  | `no-root` | `[ledger] refused: incomplete: no main! span` | 65 |
  | `gap` | `[ledger] refused: gap or duplicate at seq 3` | 65 |
  | `alien` | `[ledger] refused: seq 1 names hosted_file_delete, which is outside the boundary` | 65 |
  | `no-seq` | `[ledger] refused: line 1: not a ledger span` | 65 |
  | `bad-b64` | `[ledger] refused: line 1: not a ledger span` | 65 |
  | `bad-args` | `[ledger] refused: line 6: not a ledger span` | 65 |
  | `bad-outcome` | `[ledger] refused: line 6: not a ledger span` | 65 |
  | `no-exe` | `[ledger] refused: incomplete: no pinned binary` | 65 |
  | `no-spans` | `[ledger] refused: no spans.jsonl` | 65 |
  | `tamper` | `[ledger] divergence: Mismatch at seq 1 (hosted_stdout_line)`, then `recorded: hosted_stdout_line "TAMPERED"` and `replayed: hosted_stdout_line "Name?"` | 70 |
  | `drop-last` | `[ledger] divergence: Extra at seq 5 (hosted_stdout_line)` | 70 |
  | `add-entry` | `[ledger] divergence: Missing at seq 6 (hosted_stdout_line)` | 70 |
  | `outcome` | `[ledger] divergence: Mismatch at seq 0 (main!)`, then `recorded: returned 42` and `replayed: returned 0` | 70 |

- **Original intact.** Run `tx env -i ROC_LEDGER=replay:happy.ledger happy.ledger/exe`, which still ends `[ledger] reproduced: returned 0 after 5 crossings`.
- **Record exists.** In `user/`, run `tx env ROC_LEDGER=record:ledgers/happy.ledger ./agenda fine.txt`. It prints `[ledger] ledgers/happy.ledger already exists` and exits 73. The app never runs, and `ledgers/happy.ledger/spans.jsonl` is unchanged.
- **Bad switch.** Run `tx env ROC_LEDGER=bogus:x ./agenda`, which prints `[ledger] ROC_LEDGER must be record:<dir>, replay:<dir> or dump:<dir>` and exits 64.
- **Proof.** The transcript holds one `damage.py` line and one verdict per kind, followed by the original still reproducing.

## Gotchas

- Every divergence also prints `[ledger]   ledger: <dir>` and `This is a bug in the effect ledger: the pinned build replayed its own recording differently.` The message assumes the file wasn't edited by hand. Quote it verbatim.
- The exact numbers depend on the source ledger. `torn line 6`, `line 6: not a ledger span`, `gap … at seq 3`, `Extra at seq 5` and `Missing at seq 6` hold for the six-line `happy.ledger`. A different source shifts them, so record what printed.
- Every damaged copy must come from a real recording made in this proof. Writing `spans.jsonl` from scratch tests the fixture, not the ledger.
- Every damaged ledger ends in a refusal (65) or a divergence (70). An abort (exit 134, `panicked at src/ledger_prototype.rs`) is a bug in the replayer, not a verdict on the ledger.
- `damage.py` refuses an existing destination, so pick a new name for each attempt.
