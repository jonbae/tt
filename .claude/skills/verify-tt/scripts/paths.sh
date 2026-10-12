# Sourced by the other verify-tt scripts: the paths every recipe uses.
SCRIPTS=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$SCRIPTS/../../../.." && pwd)
PROTO=${TT_PROTO:-$ROOT/.scratch/effect-ledger/prototype-supervisor-wiring}
ROC_VERSION=nightly-2026-09-26-d6267b4
ROC=${TT_ROC:-$PROTO/roc-nightly/roc_nightly-macos_x86_64-2026-09-26-d6267b4/roc}
HOST_LIB=$PROTO/basic-cli/platform/targets/x64mac/libhost.a
PROOFS=$ROOT/.scratch/verify-tt/proofs

# Prints the PIDs running an executable that lives inside proof dir $1: its app
# builds and every ledger's pinned exe, so supervisors and children alike. A
# shell standing in the proof dir runs /bin/zsh or bash, so it never matches.
pids_in() {
	lsof -d txt -Fpn 2>/dev/null |
		awk -v d="$1" '/^p/ {p = substr($0, 2)} /^n/ {if (index(substr($0, 2), d "/") == 1) print p}' |
		sort -u
}
