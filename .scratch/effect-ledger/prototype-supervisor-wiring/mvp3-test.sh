#!/usr/bin/env bash
# MVP 3, throwaway: what did a request do? From its ledger alone. See MVP3.md.
#
# 1. record: a crash (agenda), a clean run (agenda), a Ctrl-C (two-questions)
# 2. dump:   each ledger directory only, a clean env, no app files
# 3. OTLP:   send every spans.jsonl line unchanged to Jaeger; check each root
set -u
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
mkdir -p "$work/user" "$work/elsewhere"
cp "$here/app/agenda" "$here/app/two-questions" "$work/user/"
cd "$work/user"

# The crash needs an item due this minute, at second 1 or later.
while s=$(date -u +%S); [ $((10#$s)) -lt 2 ] || [ $((10#$s)) -gt 50 ]; do sleep 1; done
now=$(date -u +%H:%M)
printf '09:00 Standup\n%s Submit essay draft\n23:59 Call home\n' "$now" > tasks.txt
printf '00:01 Early thing\n23:59 Late thing\n' > fine.txt

echo "== 1. record"
ROC_LEDGER=record:ledgers/crash.ledger ./agenda tasks.txt
ROC_LEDGER=record:ledgers/fine.ledger ./agenda fine.txt
python3 "$here/drive.py" "wait:Name?,send:Ann,wait:Age?,ctrlc" -- \
	env ROC_LEDGER=record:ledgers/ctrlc.ledger ./two-questions | tail -1

echo
echo "== 2. dump: the ledger directory only; clean env; no app files"
cp -R ledgers/*.ledger "$work/elsewhere/"
cd "$work/elsewhere"
ls -A
for l in crash fine ctrlc; do
	echo
	env -i ROC_LEDGER=dump:$l.ledger $l.ledger/exe
	echo "exit $?"
done

echo
echo "== 3. OTLP: Jaeger at localhost:4318"
"$here/otlp-send.sh" crash.ledger fine.ledger ctrlc.ledger
sleep 1
for l in crash fine ctrlc; do
	trace=$(head -1 $l.ledger/spans.jsonl | python3 -c 'import json,sys; print(json.load(sys.stdin)["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"])')
	curl -s "localhost:16686/api/traces/$trace" > $l.jaeger.json
	python3 - $l <<'EOF'
import json, sys
name = sys.argv[1]
traces = json.load(open(f"{name}.jaeger.json"))["data"]
spans = traces[0]["spans"]
roots = [s for s in spans if not s["references"]]
tags = {x["key"]: x["value"] for x in roots[0]["tags"]}
print(f"{name:6} traces {len(traces)}, spans {len(spans)}, roots {len(roots)}, "
      f"error={tags.get('error', False)}, status={tags.get('otel.status_description', 'ok')!r}")
EOF
done
echo "work dir: $work"
