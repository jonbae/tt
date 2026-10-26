# tt

Record and replay for Roc apps, plus a small app to dogfood it on.

| Path | What |
|---|---|
| [`record-replay/`](record-replay/) | the effect ledger: an `effect-ledger` crate that records every crossing between a Roc core and its platform, then replays or dumps it. It binds to forks of basic-cli and basic-webserver (submodules). |
| [`todo-app/`](todo-app/) | a SQLite todo list on basic-webserver, built against the ledger-enabled fork |

## Setup

macOS x86_64 only, for now.

```sh
git submodule update --init
```

Unpack the two Roc nightlies the forks pin into `record-replay/roc-nightly/`, which git ignores:

- `roc_nightly-macos_x86_64-2026-09-26-d6267b4` (basic-cli)
- `roc_nightly-macos_x86_64-2026-10-09-258ab27` (basic-webserver)

Then build both ledger hosts and run their tests:

```sh
.claude/skills/verify-tt/scripts/build.sh
```

`record-replay/README.md` covers the design and the proof, and `record-replay/GLOSSARY.md` the vocabulary.
