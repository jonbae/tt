#!/usr/bin/env bash
# Start a Jaeger for one proof on random loopback ports and wait until it
# answers. Writes the ports to <proof>/jaeger.env (OTLP=host:port, UI=host:port)
# because shell state doesn't survive between calls. Marks the container in
# <proof>/jaeger.container first, so cleanup.sh removes it even if this fails.
#
#   jaeger-up.sh <proof-dir>      then: . <proof-dir>/jaeger.env
set -u
proof=$(cd "${1:?usage: jaeger-up.sh <proof-dir>}" 2>/dev/null && pwd) || { echo "jaeger-up: no proof dir at $1" >&2; exit 2; }
[ -e "$proof/.verify-tt-proof" ] || { echo "jaeger-up: $proof is not a verify-tt proof dir" >&2; exit 2; }
docker info >/dev/null 2>&1 || { echo "jaeger-up: the docker daemon is down" >&2; exit 1; }
name=verify-tt-$(basename "$proof")
echo "$name" >"$proof/jaeger.container"
docker run -d --name "$name" -p 127.0.0.1::16686 -p 127.0.0.1::4318 jaegertracing/jaeger:latest >/dev/null || exit 1
otlp=$(docker port "$name" 4318/tcp | head -1)
ui=$(docker port "$name" 16686/tcp | head -1)
# Ready means the legacy trace endpoint jaeger-traces.sh reads answers in JSON;
# an unknown trace still returns {"data":null,…}. (Jaeger 2.22 dropped /api/services.)
probe="http://$ui/api/traces/00000000000000000000000000000000"
for _ in $(seq 60); do
	curl -s "$probe" | grep -q '"data"' && break
	sleep 1
done
curl -s "$probe" | grep -q '"data"' || { echo "jaeger-up: $name never answered on $probe" >&2; exit 1; }
printf 'OTLP=%s\nUI=%s\n' "$otlp" "$ui" | tee "$proof/jaeger.env"
echo "container $name"
