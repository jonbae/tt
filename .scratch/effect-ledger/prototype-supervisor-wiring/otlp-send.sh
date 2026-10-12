#!/usr/bin/env bash
# MVP 3, throwaway: send ledgers to an OTLP/HTTP collector, unchanged.
# Each spans.jsonl line is already a whole OTLP/JSON export request.
#
#   docker run -d --name ledger-jaeger -p 16686:16686 -p 4318:4318 jaegertracing/jaeger:latest
#   ./otlp-send.sh <dir>.ledger...       then open http://localhost:16686
set -u
endpoint="${OTLP_ENDPOINT:-http://localhost:4318}/v1/traces"
for dir in "$@"; do
	n=0 bad=0
	while IFS= read -r line; do
		n=$((n + 1))
		code=$(printf '%s' "$line" | curl -s -o /dev/null -w '%{http_code}' -X POST \
			-H 'Content-Type: application/json' --data-binary @- "$endpoint")
		[ "$code" = 200 ] || { bad=$((bad + 1)); echo "  line $n: HTTP $code"; }
	done < "$dir/spans.jsonl"
	echo "$dir: sent $n lines, $bad rejected"
done
