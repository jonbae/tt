#!/usr/bin/env bash
# The agenda crash needs the world at one moment: an item due in the current UTC
# minute, at second 1 or later. This sets that up, and waits for it to pass.
#
#   agenda-clock.sh due-now <file>   wait for UTC second 2-50, write <file> with an
#                                    item due this minute, print that HH:MM
#   agenda-clock.sh after <file>     wait until the UTC minute is past the item
#                                    due-now wrote into <file>
set -u
case "${1:-}" in
due-now)
	file=${2:?usage: agenda-clock.sh due-now <file>}
	while s=$(date -u +%S); [ $((10#$s)) -lt 2 ] || [ $((10#$s)) -gt 50 ]; do sleep 1; done
	now=$(date -u +%H:%M)
	printf '00:01 Early thing\n%s Submit essay draft\n23:59 Late thing\n' "$now" >"$file"
	echo "$now"
	;;
after)
	file=${2:?usage: agenda-clock.sh after <file>}
	due=$(sed -n '2s/ .*//p' "$file")
	while [ "$(date -u +%H:%M)" = "$due" ]; do sleep 1; done
	echo "past $due at $(date -u +%H:%M:%S)"
	;;
*)
	sed -n '2,8p' "$0"
	exit 64
	;;
esac
