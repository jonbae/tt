#!/usr/bin/env bash
# Rebuild the ledger host, install it into the platform, run the codec tests,
# then the doctor. Every proof dir builds its apps against this one libhost.a,
# so it is shared state: run it only while no other agent is mid-proof.
set -eu
. "$(dirname "$0")/paths.sh"
cd "$PROTO/basic-cli"
cargo build --release --lib
cp target/release/libhost.a "$HOST_LIB"
cargo test --release --lib ledger_codec
"$SCRIPTS/doctor.sh"
