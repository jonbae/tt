#!/usr/bin/env bash
# Read-only: is this checkout worth driving? Given a proof dir, also checks
# that proof's app builds and stray processes. Exit 1 when any FAIL line prints.
#
#   doctor.sh              the toolchain and the shared host
#   doctor.sh <proof-dir>  also that proof's apps and processes
set -u
. "$(dirname "$0")/paths.sh"
fail=0
ok() { printf 'ok    %s\n' "$*"; }
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
info() { printf 'info  %s\n' "$*"; }

[ "$(uname -sm)" = "Darwin x86_64" ] && ok "macOS x86_64" || bad "$(uname -sm): the host is built for x64mac only"
for t in cargo python3 curl lsof; do command -v $t >/dev/null && ok "$t" || bad "$t is not on PATH"; done
if [ -x "$ROC" ]; then
	v=$("$ROC" version 2>&1)
	case $v in *"$ROC_VERSION"*) ok "roc $ROC_VERSION" ;; *) bad "roc says '$v', expected $ROC_VERSION" ;; esac
else
	bad "no roc at $ROC"
fi
b=$(git -C "$PROTO/basic-cli" branch --show-current 2>/dev/null)
if [ "$b" = prototype/ledger-supervisor-wiring ]; then
	ok "basic-cli on $b, $(git -C "$PROTO/basic-cli" status --short | wc -l | tr -d ' ') uncommitted paths"
else
	bad "basic-cli branch is '${b:-missing}', expected prototype/ledger-supervisor-wiring"
fi
if [ -f "$HOST_LIB" ]; then
	newer=$(find "$PROTO/basic-cli/src" "$PROTO/basic-cli/Cargo.toml" "$PROTO/basic-cli/Cargo.lock" -type f -newer "$HOST_LIB" 2>/dev/null)
	[ -z "$newer" ] && ok "host is newer than every host source" || bad "host is stale, run build.sh. Newer: $(echo $newer)"
	LC_ALL=C grep -q 'record:<dir>, replay:<dir> or dump:<dir>' "$HOST_LIB" &&
		ok "host carries the ROC_LEDGER record/replay/dump switch" ||
		bad "host has no ROC_LEDGER switch, so its apps would ignore ROC_LEDGER and record nothing"
else
	bad "no host at $HOST_LIB, run build.sh"
fi
docker info >/dev/null 2>&1 && info "docker daemon up (otlp-jaeger is reachable)" || info "docker daemon down (otlp-jaeger is unreachable)"

if [ $# -ge 1 ]; then
	proof=$(cd "$1" 2>/dev/null && pwd) || { bad "no proof dir at $1"; exit 1; }
	[ -e "$proof/.verify-tt-proof" ] || bad "$proof is not a verify-tt proof dir"
	for app in agenda two-questions; do
		bin=$proof/user/$app
		if [ ! -x "$bin" ]; then bad "$app missing from $proof/user (cleaned up already?)"; continue; fi
		[ "$bin" -nt "$HOST_LIB" ] && ok "$app was built after the host" || bad "$app predates the host, make a new proof dir"
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
