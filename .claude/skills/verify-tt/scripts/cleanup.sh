#!/usr/bin/env bash
# Tear down what one proof started and drop its scratch, then confirm the
# evidence survived. Exit 1 when the evidence is incomplete.
#
#   cleanup.sh <proof-dir>
set -u
. "$(dirname "$0")/paths.sh"
proof=$(cd "${1:?usage: cleanup.sh <proof-dir>}" 2>/dev/null && pwd) || { echo "cleanup: no proof dir at $1" >&2; exit 2; }
[ -e "$proof/.verify-tt-proof" ] || { echo "cleanup: $proof is not a verify-tt proof dir" >&2; exit 2; }

# Processes this proof started: only they run a binary from inside it. A pty
# driver whose app dies sees EOF and exits by itself.
pids=$(pids_in "$proof")
for p in $pids; do echo "stop  $p  $(ps -o command= -p "$p")"; done
for p in $pids; do kill -TERM "$p" 2>/dev/null; done
[ -n "$pids" ] && sleep 1
for p in $pids; do kill -0 "$p" 2>/dev/null && kill -KILL "$p" 2>/dev/null; done

# The Jaeger container, only when this proof started it.
if [ -f "$proof/jaeger.container" ]; then
	name=$(cat "$proof/jaeger.container")
	docker rm -f "$name" >/dev/null 2>&1 && echo "removed container $name"
fi

# Scratch: the proof's own app builds. Each ledger keeps its pinned copy.
rm -f "$proof/user/agenda" "$proof/user/two-questions"

# The evidence must survive.
fail=0
[ -s "$proof/transcript.txt" ] || { echo "MISSING  $proof/transcript.txt"; fail=1; }
[ -s "$proof/build.txt" ] || { echo "MISSING  $proof/build.txt"; fail=1; }
echo "evidence kept in $proof:"
echo "  transcript.txt  $(grep -cE '^\[[0-9]{2}:[0-9]{2}:[0-9]{2}\] ' "$proof/transcript.txt" 2>/dev/null || echo 0) commands"
for l in $(find "$proof" -name '*.ledger' -type d | sort); do
	spans=$([ -f "$l/spans.jsonl" ] && echo "$(wc -l <"$l/spans.jsonl" | tr -d ' ') lines" || echo none)
	exe=$([ -x "$l/exe" ] && echo pinned || echo none)
	echo "  ${l#"$proof"/}  spans.jsonl: $spans, exe: $exe"
done
for f in $(find "$proof" -maxdepth 3 -type f \( -name '*.txt' -o -name '*.json' \) ! -name transcript.txt ! -name build.txt | sort); do
	echo "  ${f#"$proof"/}"
done
exit $fail
