# record-replay: the effect ledger (prototype)

Asks whether the ledger can be a library that any Rust platform host links, and binds it to two hosts to find out: basic-cli `0.23.0` (one request per process) and basic-webserver `0.18.0` (many concurrent requests per process). Built on macOS x86_64. basic-cli uses Roc `nightly-2026-09-26-d6267b4` and basic-webserver uses `nightly-2026-10-09-258ab27`.

Answer: yes. The supervisor, wire, OTLP file, pinning, replay matcher and dump are one crate with no platform code in it. A host hands it one table and wraps its crossings. Making the server work took one change to the design: the request is now explicit, instead of being the process.

## What's here

| Path | What |
|---|---|
| `GLOSSARY.md` | the vocabulary: request, crossing, instance, outcome, replay, divergence |
| `effect-ledger/` | the library: `start`, `enter`/`leave`, `cross`/`record`, `crashed`, `serve_replay`, and the `Boundary` table type |
| `basic-cli/` | submodule: [jonbae/basic-cli](https://github.com/jonbae/basic-cli), branch `prototype/ledger-library`. `ledger.rs` holds the codecs, display and `BOUNDARY` |
| `basic-webserver/` | submodule: [jonbae/basic-webserver](https://github.com/jonbae/basic-webserver), a fork of roc-lang/basic-webserver at `0.18.0`, branch `prototype/ledger-library`. `ledger.rs` is new, and 7 files gained 82 lines |
| `examples/` | `agenda.roc` and `two_questions.roc` from MVP 1, plus `agenda-server.roc`, the same agenda bug served over HTTP |
| `prove.sh` | the whole test on both platforms. It writes `proofs/<UTC>-<pid>/` |
| `roc-nightly/` | the two Roc nightlies above, unpacked. Ignored by git |
| `docs/` | research, issues and design notes. Ignored by git |
| `demo/` | built apps and a hand-recorded ledger, `app.ledger`, with a crash and a restart. Ignored by git |

Both forks depend on `effect-ledger = { path = "../effect-ledger" }`, so they build only as siblings of `effect-ledger/`, here.

## What a platform writes

The library reads one table. Record, replay and dump take every platform fact from it:

```rust
pub static BOUNDARY: Boundary = Boundary {
    platform: "basic-webserver 0.18.0",
    hosted: &[Hosted { symbol: "hosted_unix_time_now", echo: Echo::Nothing, show_request, show_response }, ...],
    entries: &[
        Entry { name: "init!", after: &[], replay: replay_init, ok, show_request, show_response },
        Entry { name: "respond!", after: &["init!"], replay: replay_respond, ok, show_request, show_response },
        Entry { name: "shutdown!", after: &["init!"], replay: replay_shutdown, ok, show_request, show_response },
    ],
};
```

Then it touches three places in the host:

1. `rust_main` calls `effect_ledger::start(&BOUNDARY, &args)` first and returns if that gives a code. After host setup, it calls `effect_ledger::serve_replay()` if `effect_ledger::replaying()`.
2. Each host-to-core call is bracketed: `let r = effect_ledger::enter("respond!", || encode_request(&req));` before it, and `r.leave(|| encode_response(&resp))` after it.
3. Each hosted function starts with `effect_ledger::cross(SYMBOL, || request_bytes)`. On `Cross::Replay(bytes)` it releases its arguments and decodes. Otherwise it performs the effect and calls `.record(|| response_bytes)`.

The `hosted` rows and the wrappers are what codegen from `Host.roc` would emit. The `entries` stay hand-written, because re-entering the core needs the platform's own types. basic-webserver's `init!` replay keeps the context for the `respond!` that follows, and its `respond!` replay builds a `Server.Request` with a fresh box standing in for the body capability.

## What changed in the design

MVP 1 treated the process as the request: one global sequence counter, a `main!` root written at exit, and replay by re-running the process with the recorded args. A server breaks all three. So:

- The child names the request in every frame: `ENTER(rid, entry, request)`, `BEGIN(rid, seq, …)`, `END(rid, seq, …)`, `LEAVE(rid, response)`. The sequence number lives in a thread-local, so concurrent requests never share a counter. Roc calls are synchronous, so a request's crossings all happen on the thread that entered it. The pipe is the one shared thing, and it takes one write per frame under a lock.
- The supervisor keeps a table of open requests. Each request gets its own trace id and its own root span, written when it leaves. When the process dies, the request that crashed gets `crashed`. Every other open request gets `process exited N during seq K`, or `killed by SIG…`.
- Replay sends `RUN(entry, request)` frames, and the child re-enters the core through the entry's `replay` fn. A request is replayed after its `after` prerequisites, in a fresh child per request. `ROC_LEDGER_REQUEST=<trace id prefix>` replays one request.
- basic-cli's `main!` is now one entry among others. Its request is the process args, and its response is the exit code.

Then a ledger had to outlive a crash. A real server dies and gets restarted, and its ledger should hold both processes. So a ledger now holds instances (GLOSSARY.md):

```
app.ledger/
  .pins/<build>                       one copy per distinct build
  20261015T143503Z-f85f/exe           this instance's pinned binary
  20261015T143503Z-f85f/spans.jsonl   written by this process only
  20261015T143503Z-f85f/instance.json pid, start, and how it exited
  20261015T143504Z-9997/...           the restart
```

- Recording into an existing ledger adds an instance instead of refusing. A restart, or two replicas recording at once, never write the same file.
- Every ledger entry's OTLP resource carries `service.instance.id` and `process.pid`.
- Replay and dump of a ledger run each instance through its own pinned binary, so instances recorded by different builds each decode their own entries. A request's `init!` comes from its own instance.
- `dump` prints one timeline for the whole ledger, ordered by time, with an instance and a request column. Each process's start and exit are rows of their own. `ROC_LEDGER_REQUEST=<prefix>` narrows it to one request.
- The effect a process died in is its own ledger entry, at the time it began, with no response and an error status. Before, it lived only inside the request's outcome.
- A crashed request ends when its crash arrives, so it sorts ahead of the exit it caused.

## Run

```sh
git submodule update --init
(cd basic-cli && cargo build --release --lib && mkdir -p platform/targets/x64mac && cp target/release/libhost.a platform/targets/x64mac/)
(cd basic-webserver && cargo build --release --lib && mkdir -p platform/targets/x64mac && cp target/release/libhost.a platform/targets/x64mac/)
(cd effect-ledger && cargo test) && (cd basic-cli && cargo test --release --lib ledger) && (cd basic-webserver && cargo test --release --lib ledger)
./prove.sh
```

By hand, from `demo/`. Start the server, send it requests from another terminal, and press Ctrl-C. Run it again into the same ledger to add an instance.

```sh
ROC_LEDGER=record:mine.ledger ./agenda-server
curl localhost:8137/agenda; curl localhost:8137/slow & curl localhost:8137/slow & wait
ROC_LEDGER=dump:mine.ledger ./agenda-server                               # one timeline
ROC_LEDGER=dump:mine.ledger ROC_LEDGER_REQUEST=<request> ./agenda-server  # one request
ROC_LEDGER=replay:mine.ledger ./agenda-server
```

Any instance's `exe` can stand in for `./agenda-server`, so the ledger directory alone is enough. `../../.claude/skills/verify-tt/scripts/agenda-clock.sh due-now tasks.txt` adds a task due this minute, which makes `/agenda` crash until the minute passes.

## Result

`proofs/20261015T143617Z-71341/`, 28 of 28 claims ok. Excerpts are verbatim from `transcript.txt`.

The crash, recorded on a live server with two `/slow` requests in flight:

```
GET /slow begin
GET /slow begin
GET /agenda at 52585s
[ROC CRASHED] Integer subtraction overflowed
[ledger] recorded 4 request(s) (shared) -> ledgers/web-crash.ledger, instance 20261015T143624Z-3af7
[ledger]   respond! 86ee3bb1: process exited 1 during seq 2 (hosted_sleep_millis)
[ledger]   respond! ea612678: process exited 1 during seq 2 (hosted_sleep_millis)
[ledger]   respond! f2c036bc: crashed: Integer subtraction overflowed
server exit 1
```

The server then restarted into the same ledger, served one `/slow`, and got SIGTERM. The dump of a copy, with `env -i` and no `tasks.txt`, reads as one timeline across both processes:

```
time                         inst  request     seq
2026-10-15T14:36:25.423824Z  3af7  86ee3bb1  begin  respond!(GET /slow, 3 headers)
2026-10-15T14:36:25.423850Z  3af7  ea612678  begin  respond!(GET /slow, 3 headers)
2026-10-15T14:36:25.423854Z  3af7  ea612678      1  hosted_stdout_line("GET /slow begin") -> Ok({})
2026-10-15T14:36:25.423864Z  3af7  86ee3bb1      1  hosted_stdout_line("GET /slow begin") -> Ok({})
2026-10-15T14:36:25.423967Z  3af7  ea612678      2  hosted_sleep_millis(300) -> no response: process exited 1
2026-10-15T14:36:25.424024Z  3af7  86ee3bb1      2  hosted_sleep_millis(300) -> no response: process exited 1
2026-10-15T14:36:25.542175Z  3af7  f2c036bc  begin  respond!(GET /agenda, 3 headers)
2026-10-15T14:36:25.542181Z  3af7  f2c036bc      1  hosted_file_read_utf8({ unix: "tasks.txt" }) -> Ok("00:01 Early thing\n14:36 Submit essay draft\n23:59 Late thing\n")
2026-10-15T14:36:25.542343Z  3af7  f2c036bc      2  hosted_unix_time_now() -> 1792074985542200000  # 2026-10-15T14:36:25.542200Z
2026-10-15T14:36:25.542398Z  3af7  f2c036bc      3  hosted_stdout_line("GET /agenda at 52585s") -> Ok({})
2026-10-15T14:36:25.542517Z  3af7  f2c036bc    end  crashed: Integer subtraction overflowed
2026-10-15T14:36:25.543961Z  3af7  86ee3bb1    end  process exited 1 during seq 2 (hosted_sleep_millis)
2026-10-15T14:36:25.544505Z  3af7  ea612678    end  process exited 1 during seq 2 (hosted_sleep_millis)
2026-10-15T14:36:25.545065Z  3af7             exit  process exited 1
2026-10-15T14:36:25.593279Z  5b8a            start  agenda-server (basic-webserver 0.18.0), pid 71638
2026-10-15T14:36:25.684946Z  5b8a  0621e9ad  begin  init!()
```

The crashed request alone, replayed from the same copy:

```
elsewhere$ env -i ROC_LEDGER=replay:web-crash.ledger ROC_LEDGER_REQUEST=f2c036bc web-crash.ledger/20261015T143624Z-3af7/exe
[ledger] instance 20261015T143624Z-3af7
GET /agenda at 52585s
[ROC CRASHED] Integer subtraction overflowed
[ledger] reproduced respond! f2c036bc: crashed: Integer subtraction overflowed after 3 crossings
exit 0
```

With the first instance's `init!` damaged, the restarted instance's `/slow` still replays, and the crashed request fails on its own instance's `init!`:

```
elsewhere$ env -i ROC_LEDGER=replay:web-scope.ledger ROC_LEDGER_REQUEST=4d7ca7fd web-scope.ledger/20261015T143624Z-3af7/exe
[ledger] instance 20261015T143625Z-5b8a
[ledger] reproduced respond! 4d7ca7fd: returned 200 [Content-Type: text/plain; charset=utf-8] "slow 52586" after 4 crossings
elsewhere$ env -i ROC_LEDGER=replay:web-scope.ledger ROC_LEDGER_REQUEST=f2c036bc web-scope.ledger/20261015T143624Z-3af7/exe
[ledger] divergence in respond! f2c036bc: while replaying init! b84bbff5 first: Mismatch at seq 1 (hosted_stdout_line)
exit 70
```

A clean session recorded 7 requests (`init!`, `/agenda`, a 404, three overlapping `/slow`, `shutdown!`), and all 7 replayed with byte-identical HTTP responses. The same run reproduced basic-cli's agenda crash and a Ctrl-C at `Age?`. The controls showed that the same `tasks.txt`, after its minute had passed, crashed neither platform. A changed file response in a `/agenda` crossing changed the replayed HTTP body, so replay reported `Mismatch at seq 0 (respond!)` and exited 70.

## Findings

- **The library needs requests to be independent.** Replaying one `respond!` re-runs only `init!`. That's sound only because basic-webserver gives each handler immutable context and keeps every other piece of state behind a hosted function (design.md, "Architectural boundaries"). A platform that keeps mutable state in the core across requests would have to replay every earlier request. So "any host" really means any host whose requests depend only on named earlier requests.
- **Dependencies by entry name are too coarse for SSE.** `roc_sse_advance_for_host!` runs a Roc closure that an earlier `respond!` returned, on a worker thread, outside any request. From the code, not a run: recording it today would end the process with exit 70 (`crossed outside any request`) as soon as the stream's step makes a crossing. Replaying it would need the specific `respond!` that returned the stream, not "the latest `respond!`".
- **Unwrapped hosted functions run for real during replay.** The boundary covers only the 5 wrapped functions on each platform. Codegen that wraps all of them closes this, and it's the open *Layout-table code generation* item in the map.
- **Handles still need logical ids.** `Server.Request`'s body capability is a heap address, so the `respond!` request encoding leaves it out, and replay passes a fresh box. A body read would cross through a wrapped hosted function, but the handle in its request would differ on every run.
- **`init!`'s response records only Ok or the exit code.** The returned config isn't compared on replay.
- **Not measured: what recording costs a server.** Every worker thread writes frames into one pipe under one lock, and one supervisor thread writes every span. basic-webserver's design.md asks the request path to avoid global serialization. This prototype has one.
- **Row times run late.** A row's time is when the supervisor read the frame, not when the effect happened. In this proof's ledgers, all six `hosted_unix_time_now` rows are stamped 0.06 to 0.17 ms after the clock value the app read. Stamping the time in the app, inside `cross`, would fix it.
- **A merged dump costs one process per instance.** Each instance's pinned binary decodes its own rows, because a deploy can change the build between instances. Not measured for a ledger with many instances.
- **The first instance of a new build starts slowly on macOS.** In `demo/`, a basic-cli instance took 297 ms from start to `main!` when its build was new, and 5 ms for the next instance of the same build. That is the first-exec cost `pin.rs` already notes, paid once per build.
