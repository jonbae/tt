#!/usr/bin/env bash
# Test the effect-ledger crate, rebuild both ledger hosts, install them into
# their platforms, run their codec tests, then the doctor. Every proof dir
# builds its apps against these libhost.a files, so they are shared state: run
# this only while no other agent is mid-proof.
set -eu
. "$(dirname "$0")/paths.sh"
(cd "$PROTO/effect-ledger" && cargo test)
for p in $PLATFORMS; do
	cd "$PROTO/$p"
	cargo build --release --lib
	mkdir -p "$(dirname "$(host_lib "$p")")"
	cp target/release/libhost.a "$(host_lib "$p")"
	cargo test --release --lib ledger
done
"$SCRIPTS/doctor.sh"
