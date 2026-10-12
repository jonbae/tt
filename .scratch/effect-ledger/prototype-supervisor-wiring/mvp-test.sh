#!/usr/bin/env bash
# MVP, throwaway: the effect-ledger MVP test, end to end. See MVP.md.
#
# 1. user:      always-on recording; the agenda app crashes
# 2. control:   same file, a minute later: no crash, so the file alone isn't enough
# 3. elsewhere: only the ledger directory, a clean env, no agenda file: same crash
# 4. cost:      time with and without recording, and the ledger's size
set -u
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
user="$work/user"
elsewhere="$work/elsewhere"
mkdir -p "$user" "$elsewhere"
cp "$here/app/agenda" "$user/agenda"
. "$here/ledger-on.sh"
export ROC_LEDGER_DIR="$user/.ledgers"
cd "$user"

# The bug needs an item due this minute, at second 1 or later.
while s=$(date -u +%S); [ $((10#$s)) -lt 2 ] || [ $((10#$s)) -gt 50 ]; do sleep 1; done
now=$(date -u +%H:%M)
printf '00:01 Early thing\n%s Submit essay draft\n23:59 Late thing\n' "$now" > tasks.txt

echo "== 1. user: always-on recording ($now UTC)"
ledgered ./agenda tasks.txt
echo "exit $?"
ledger=$(ls -d "$ROC_LEDGER_DIR"/*.ledger)

echo
echo "== 2. control: same tasks.txt, after $now UTC has passed"
while [ "$(date -u +%H:%M)" = "$now" ]; do sleep 1; done
./agenda tasks.txt
echo "exit $?"

echo
echo "== 3. elsewhere: the ledger directory only; clean env; no tasks.txt"
cp -R "$ledger" "$elsewhere/bug.ledger"
cd "$elsewhere"
rm -rf "$user/tasks.txt"
ls -A
env -i ROC_LEDGER=replay:bug.ledger bug.ledger/exe
echo "exit $?"

echo
echo "== 4. cost (agenda with nothing due this minute, 20 times each)"
cd "$user"
printf '00:01 Early thing\n23:59 Late thing\n' > fine.txt
t0=$(perl -MTime::HiRes=time -e 'printf "%.6f", time')
for _ in $(seq 20); do ./agenda fine.txt > /dev/null; done
t1=$(perl -MTime::HiRes=time -e 'printf "%.6f", time')
for _ in $(seq 20); do ledgered ./agenda fine.txt > /dev/null 2>&1; done
t2=$(perl -MTime::HiRes=time -e 'printf "%.6f", time')
perl -e 'printf "off: %.1f ms/request\non:  %.1f ms/request\n", ($ARGV[1]-$ARGV[0])*50, ($ARGV[2]-$ARGV[1])*50' "$t0" "$t1" "$t2"
one=$(ls -d "$ROC_LEDGER_DIR"/*.ledger | tail -1)
echo "one ledger: spans.jsonl $(wc -c < "$one/spans.jsonl" | tr -d ' ') bytes; exe $(wc -c < "$one/exe" | tr -d ' ') bytes, stored once per build ($(stat -f %l "$one/exe") links to the shared pin)"
echo "ledgers recorded in this test: $(ls -d "$ROC_LEDGER_DIR"/*.ledger | wc -l | tr -d ' ')"
echo "work dir: $work"
