#!/usr/bin/env bash
# Read-only: is this checkout worth driving? Given a proof dir, also checks
# that proof's app builds and stray processes. Exit 1 when any FAIL line prints.
#
#   doctor.sh              the toolchain and the shared hosts
#   doctor.sh <proof-dir>  also that proof's apps and processes
set -u
. "$(dirname "$0")/paths.sh"
fail=0
ok() { printf 'ok    %s\n' "$*"; }
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
info() { printf 'info  %s\n' "$*"; }

[ "$(uname -sm)" = "Darwin x86_64" ] && ok "macOS x86_64" || bad "$(uname -sm): the hosts are built for x64mac only"
for t in cargo python3 curl lsof; do command -v $t >/dev/null && ok "$t" || bad "$t is not on PATH"; done
for p in $PLATFORMS; do
	roc=$(roc_for "$p")
	want=$(roc_version "$p")
	if [ -x "$roc" ]; then
		v=$("$roc" version 2>&1)
		case $v in *"$want"*) ok "$p: roc $want" ;; *) bad "$p: roc says '$v', expected $want" ;; esac
	else
		bad "$p: no roc at $roc"
	fi
	if grep -q '^effect-ledger = ' "$PROTO/$p/Cargo.toml" 2>/dev/null; then
		ok "$p at $(git -C "$PROTO/$p" rev-parse --short HEAD) links effect-ledger, $(git -C "$PROTO/$p" status --short | wc -l | tr -d ' ') uncommitted paths"
	else
		bad "$p doesn't link effect-ledger: run git submodule update --init"
	fi
	lib=$(host_lib "$p")
	if [ -f "$lib" ]; then
		newer=$(find "$PROTO/$p/src" "$PROTO/$p/Cargo.toml" "$PROTO/$p/Cargo.lock" "$PROTO/effect-ledger/src" "$PROTO/effect-ledger/Cargo.toml" -type f -newer "$lib" 2>/dev/null)
		[ -z "$newer" ] && ok "$p host is newer than every host source" || bad "$p host is stale, run build.sh. Newer: $(echo $newer)"
		LC_ALL=C grep -q 'record:<dir>, replay:<dir> or dump:<dir>' "$lib" &&
			ok "$p host carries the ROC_LEDGER record/replay/dump switch" ||
			bad "$p host has no ROC_LEDGER switch, so its apps would ignore ROC_LEDGER and record nothing"
	else
		bad "no $p host at $lib, run build.sh"
	fi
done
docker info >/dev/null 2>&1 && info "docker daemon up (otlp-jaeger is reachable)" || info "docker daemon down (otlp-jaeger is unreachable)"

if [ $# -ge 1 ]; then
	proof=$(cd "$1" 2>/dev/null && pwd) || { bad "no proof dir at $1"; exit 1; }
	[ -e "$proof/.verify-tt-proof" ] || bad "$proof is not a verify-tt proof dir"
	for spec in $APPS; do
		app=${spec%%:*}
		platform=${spec##*:}
		bin=$proof/user/$app
		if [ ! -x "$bin" ]; then bad "$app missing from $proof/user (cleaned up already?)"; continue; fi
		[ "$bin" -nt "$(host_lib "$platform")" ] && ok "$app was built after the $platform host" || bad "$app predates the $platform host, make a new proof dir"
		out=$(ROC_LEDGER=doctor:x "$bin" 2>&1)
		code=$?
		case "$code:$out" in
		64:*"dump:<dir>"*) ok "$app answers ROC_LEDGER (exit 64 on a bad mode)" ;;
		*) bad "$app ignored ROC_LEDGER=doctor:x (exit $code), so it is not ledger-aware" ;;
		esac
	done
	pids=$(pids_in "$proof")
	[ -z "$pids" ] && ok "no processes left inside the proof dir" ||
		info "still running inside the proof dir: $(echo $pids), and cleanup.sh stops them"
fi
exit $fail
