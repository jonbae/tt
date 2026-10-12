# tt verification map

This directory is the maintained source for verifying what a developer sees when they use the effect ledger. Read this index first, then use the matching feature file as the recipe. `../SKILL.md` covers launch, the doctor, evidence and cleanup.

## Baseline preconditions

- `"$S/doctor.sh"` is all ok, and the host isn't stale.
- `PROOF=$("$S/new-proof.sh" <feature-id>)` made a fresh proof dir with `agenda` and `two-questions` built into `$PROOF/user/`, and `"$S/doctor.sh" "$PROOF"` is all ok.
- Every shell call starts with `. <proof dir>/env.sh`.
- Drive only binaries and ledgers inside your own proof dir. The ledgers in `effect-ledger-demo/` and `$PROTO/ledgers/` belong to the user, so copy them into `$PROOF/elsewhere/` before using one.

## Driving conventions

- Run every command through `tx`, from `$PROOF/user` (the world) or `$PROOF/elsewhere` (the world taken away).
- Pass environment variables through `env` so the transcript shows them. Replays in `elsewhere/` run under `env -i`.
- Give each ledger a name that's new within the proof, because recording refuses an existing directory.
- Drive interactive requests with `$PROTO/drive.py` through a real pty, and read the app's end from its `<< shell sees: … >>` line.
- Treat the `[ledger] …` lines as the ledger's verdict on itself, and the app's output as what happened. Leave whether the app's output is right to the developer.

## Proof and skip reporting

- A recording proof includes the app's output, the `[ledger] recorded …` line, the exit status the shell saw, and the ledger directory left in the proof.
- A replay proof was run in `elsewhere/` under `env -i`, and shows the replayed output, the verdict line and the exit code.
- A reproduction proof also carries its control: a live rerun after the world changed, with its different output.
- Report the proof dir, the feature and sub-feature IDs, and the verbatim transcript lines for each claim.
- Report an unreachable path with the attempted command and the unmet precondition. A skipped sub-feature stays unverified even when a neighbouring one passed.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the developer-visible behavior. It then has exactly four H2 sections, in this order:

1. `Sub-features` lists short IDs with one line for each behavior.
2. `How to get to it (user POV)` lists every entry point a developer uses.
3. `Driving it with tx` starts with `Preconditions:` and pairs each action with an exact command and its observable result.
4. `Gotchas` lists traps that waste or invalidate a proof.

## Features

- [Record and replay](./record-replay.md) covers recording a request, the control, replaying with the world taken away, always-on recording and shared pins.
- [Interactive requests and signals](./interactive-and-signals.md) covers stdin through a pty, Ctrl-C, Ctrl-D, SIGTERM and interrupted replays.
- [Damaged ledgers](./damaged-ledgers.md) covers refusals (65), divergence (70), and records into an existing directory (73).
- [Dump](./dump.md) covers reading what a request did from its ledger alone, including damaged and pre-dump ledgers.
- [OTLP export to Jaeger](./otlp-jaeger.md) covers sending the ledger's spans unchanged to a stock trace viewer.
