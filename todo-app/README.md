# todo-app

A SQLite todo list on basic-webserver. It exists to dogfood [record-replay](../record-replay/), so it's built against the ledger-enabled fork in `../record-replay/basic-webserver/`, not a released platform.

## Build and run

Follow the setup in the [root README](../README.md) first: it installs the ledger host this app links.

```sh
ROC=../record-replay/roc-nightly/roc_nightly-macos_x86_64-2026-10-09-258ab27/roc
$ROC build main.roc --output=todo-app
./todo-app
```

Open http://127.0.0.1:8000. The todos live in `todos.db` in the working directory.

## Record, dump and replay

```sh
ROC_LEDGER=record:session.ledger ./todo-app     # use the app, then Ctrl-C
ROC_LEDGER=dump:session.ledger ./todo-app       # every request, with its crossings
ROC_LEDGER=replay:session.ledger ./todo-app     # every request again, with the world taken away
```

Recording into the same ledger again adds an instance. `ROC_LEDGER_REQUEST=<request id prefix>` narrows a replay or dump to one request. `../record-replay/README.md` covers the rest.

## Prove

`./prove.sh` records one session (add two todos, toggle one, delete one, load the page) against a real database. It then copies only the ledger elsewhere, replays it with no database, and prints one line per claim. Each run leaves its transcript and ledgers in `proofs/`. It checks that:

- every request reproduces, and the last page replays alone, byte for byte
- replay never opens a database
- `dump` shows the SQL the session ran and the rows SQLite returned
- a changed SQLite row changes the page, so replay reports a divergence
- a crossing recorded on a different handle (`init!#9` for `init!#1`) diverges

The SQLite handles in a dump are named by the crossing that created them: `init!#1` is the database `init!` opened, and `respond!#3` is the statement seq 3 of that request prepared.
