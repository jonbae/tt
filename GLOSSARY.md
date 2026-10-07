# Effect Ledger

The vocabulary for recording, observing, and replaying everything that crosses between a pure Roc core and its effectful platform.

## Language

**Effect**:
One call to a hosted function: the core asking the platform to do something in the world.
_Avoid_: Side effect, IO call, syscall

**Effect request**:
The arguments an effect passes to its hosted function.
_Avoid_: Request (on its own), args, input

**Effect response**:
The result an effect gets back from its hosted function.
_Avoid_: Response (on its own), return value, output

**Request**:
One host-to-core crossing and every effect made while handling it. In basic-cli, one run of `main!`.
_Avoid_: Run, session, trace

**Hosted function**:
A function the platform implements in host code, identified by the symbol string the platform header binds it to. Two hosted functions with identical declarations but different symbols are different hosted functions.
_Avoid_: Effect kind, effectful function, FFI function

**Crossing**:
One effect request paired with its effect response, as it passes over the boundary. Host-to-core entries (`main!` in basic-cli; `init!`, `respond!`, `shutdown!` in a server platform) are crossings too, in the opposite direction.
_Avoid_: Event, call, invocation

**Ledger entry**:
The stored record of a single crossing. Every ledger entry is also an OTEL span; there is no separate record.
_Avoid_: Log line, trace event, record

**Effect ledger**:
The append-only sequence of ledger entries for a running application.
_Avoid_: Audit log, event log, trace store

**Pinned binary**:
The copy of the executable that recorded an effect ledger, kept with the ledger. The recording runs from this copy, and replay only ever runs the pinned binary.
_Avoid_: Pin (on its own), snapshot, build hash

**Boundary**:
The complete, finitely enumerable set of hosted functions a given platform version exposes.
_Avoid_: Interface, API surface, FFI layer

**Sequence number**:
The position of a crossing within one request, counted in the order the crossings happened. Replay matches recorded and replayed crossings by sequence number.
_Avoid_: Effect index, slot, offset

**Outcome**:
How a request ended: the code the core returned, the message it crashed with, or the signal that killed it, together with the effect that was in flight, if any. It is the effect response of the request's host-to-core crossing.
_Avoid_: Result, exit status, termination

**Replay**:
Running a request again with every effect response taken from its effect ledger instead of from the world.
_Avoid_: Playback, rerun, simulation

**Divergence**:
A crossing at which a replay stops matching its effect ledger. Either the ledger has a crossing the replay didn't make (Missing), the replay made one the ledger doesn't have (Extra), or both have one and they differ (Mismatch).
_Avoid_: Desync, drift, nondeterminism error
