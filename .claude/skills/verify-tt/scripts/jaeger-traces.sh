#!/usr/bin/env bash
# Read each ledger's trace back from Jaeger's query API, save Jaeger's JSON next
# to the ledger as <name>.jaeger.json, and print what Jaeger holds for it.
# Jaeger indexes asynchronously, so each read retries for up to 15 s.
#
#   jaeger-traces.sh <query-host:port> <dir>.ledger...
set -u
ui=${1:?usage: jaeger-traces.sh <query-host:port> <dir>.ledger...}
shift
for dir in "$@"; do
	dir=${dir%/}
	name=$(basename "$dir" .ledger)
	out=$(dirname "$dir")/$name.jaeger.json
	trace=$(head -1 "$dir/spans.jsonl" | python3 -c 'import json,sys; print(json.load(sys.stdin)["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"])')
	for _ in $(seq 15); do
		curl -s "http://$ui/api/traces/$trace" >"$out"
		grep -q '"spans"' "$out" && break
		sleep 1
	done
	python3 - "$out" "$name" "$trace" <<'EOF'
import json, sys
path, name, trace = sys.argv[1:]
traces = json.load(open(path)).get("data") or []
if not traces:
    print(f"{name}: Jaeger has no trace {trace}")
    sys.exit(0)
spans = traces[0]["spans"]
roots = [s for s in spans if not s["references"]]
tags = {t["key"]: t["value"] for t in (roots[0]["tags"] if roots else [])}
print(f"{name}: trace {trace}, traces {len(traces)}, spans {len(spans)}, roots {len(roots)}, "
      f"error={tags.get('error', False)}, status={tags.get('otel.status_description', 'ok')!r}")
EOF
done
