#!/usr/bin/env bash
# Run one command from inside a proof dir, show its combined stdout and stderr,
# and append the command, that output, and how it ended to <proof>/transcript.txt.
# Exits the way the command did.
#
#   cd "$PROOF/user" && transcript.sh <command...>     (env.sh defines tx for this)
set -u
d=$PWD
while [ "$d" != / ] && [ ! -e "$d/.verify-tt-proof" ]; do d=$(dirname "$d"); done
if [ ! -e "$d/.verify-tt-proof" ]; then
	echo "transcript.sh: $PWD is not inside a verify-tt proof dir" >&2
	exit 2
fi
log=$d/transcript.txt
where=${PWD#"$d"}
where=${where#/}
{
	printf '\n[%s] %s$ ' "$(date -u +%H:%M:%S)" "${where:-.}/"
	printf '%q ' "$@"
	printf '\n'
} >>"$log"
"$@" 2>&1 | tee -a "$log"
code=${PIPESTATUS[0]}
if [ "$code" -gt 128 ]; then
	end="exit $code (killed by SIG$(kill -l $((code - 128))))"
else
	end="exit $code"
fi
echo "$end" | tee -a "$log"
exit "$code"
