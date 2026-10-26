#!/usr/bin/env bash
# Make a proof dir and build the demo apps into its user/ against the current
# hosts. Prints only the proof dir; the build log goes to <proof>/build.txt.
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
built=1
{
	echo "proof          $proof"
	echo "made           $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "effect-ledger  $(cd "$PROTO/effect-ledger" && shasum -a 256 src/*.rs | shasum -a 256 | cut -c1-16) (src)"
	for p in $PLATFORMS; do
		echo "$p $(git -C "$PROTO/$p" rev-parse --short HEAD), roc $("$(roc_for "$p")" version), uncommitted:"
		git -C "$PROTO/$p" status --short | sed 's/^/    /'
		echo "    host $(shasum -a 256 "$(host_lib "$p")" | cut -c1-16)"
	done
	cd "$PROTO/examples" || exit 1
	for spec in $APPS; do
		IFS=: read -r app src platform <<<"$spec"
		"$(roc_for "$platform")" build "$src" --output="$proof/user/$app" || exit 1
	done
	shasum -a 256 "$proof"/user/*
} >"$proof/build.txt" 2>&1 || built=0
for spec in $APPS; do [ -x "$proof/user/${spec%%:*}" ] || built=0; done
if [ $built = 0 ]; then
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
