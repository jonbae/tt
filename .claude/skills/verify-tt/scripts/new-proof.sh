#!/usr/bin/env bash
# Make a proof dir and build both demo apps into its user/ against the current
# host. Prints only the proof dir; the build log goes to <proof>/build.txt.
#
#   PROOF=$(new-proof.sh <feature-id>)
set -u
. "$(dirname "$0")/paths.sh"
feature=${1:?usage: new-proof.sh <feature-id>}
if ! "$SCRIPTS/doctor.sh" >/dev/null; then
	"$SCRIPTS/doctor.sh" >&2
	echo "new-proof: the doctor failed; fix that first" >&2
	exit 1
fi
proof=$PROOFS/$(date -u +%Y%m%dT%H%M%SZ)-$feature-$$
mkdir -p "$proof/user" "$proof/elsewhere"
touch "$proof/.verify-tt-proof"
{
	echo "proof      $proof"
	echo "made       $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "roc        $("$ROC" version)"
	echo "basic-cli  $(git -C "$PROTO/basic-cli" rev-parse --short HEAD) on $(git -C "$PROTO/basic-cli" branch --show-current), uncommitted:"
	git -C "$PROTO/basic-cli" status --short | sed 's/^/             /'
	echo "host       $(shasum -a 256 "$HOST_LIB" | cut -c1-16)  $HOST_LIB"
	cd "$PROTO/app" &&
		"$ROC" build agenda.roc --output="$proof/user/agenda" &&
		"$ROC" build two_questions.roc --output="$proof/user/two-questions" &&
		shasum -a 256 "$proof/user/agenda" "$proof/user/two-questions"
} >"$proof/build.txt" 2>&1
if [ ! -x "$proof/user/agenda" ] || [ ! -x "$proof/user/two-questions" ]; then
	cat "$proof/build.txt" >&2
	echo "new-proof: the app build failed; see above" >&2
	exit 1
fi
cat >"$proof/env.sh" <<EOF
# Source at the start of every shell call: . $proof/env.sh
PROOF='$proof'
ROOT='$ROOT'
S='$SCRIPTS'
PROTO='$PROTO'
tx() { "\$S/transcript.sh" "\$@"; }
EOF
echo "$proof"
