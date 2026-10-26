# Sourced by the other verify-tt scripts: the paths every recipe uses.
SCRIPTS=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$SCRIPTS/../../../.." && pwd)
PROTO=${TT_PROTO:-$ROOT/record-replay}
NIGHTLIES=${TT_NIGHTLIES:-$PROTO/roc-nightly}
PROOFS=$PROTO/proofs

# Each platform pins its own Roc nightly and installs its own ledger host.
PLATFORMS="basic-cli basic-webserver"
roc_version() {
	case $1 in
	basic-cli) echo nightly-2026-09-26-d6267b4 ;;
	basic-webserver) echo nightly-2026-10-09-258ab27 ;;
	esac
}
roc_for() {
	echo "$NIGHTLIES/roc_nightly-macos_x86_64-$(roc_version "$1" | sed 's/^nightly-//')/roc"
}
host_lib() {
	echo "$PROTO/$1/platform/targets/x64mac/libhost.a"
}

# The demo apps: <app> <source in $PROTO/examples> <platform>.
APPS="agenda:agenda.roc:basic-cli two-questions:two_questions.roc:basic-cli agenda-server:agenda-server.roc:basic-webserver"

# Prints the PIDs running an executable that lives inside proof dir $1: its app
# builds and every ledger's pinned exe, so supervisors and children alike. A
# shell standing in the proof dir runs /bin/zsh or bash, so it never matches.
pids_in() {
	lsof -d txt -Fpn 2>/dev/null |
		awk -v d="$1" '/^p/ {p = substr($0, 2)} /^n/ {if (index(substr($0, 2), d "/") == 1) print p}' |
		sort -u
}
