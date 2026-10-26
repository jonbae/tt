#!/usr/bin/env bash
# PROTOTYPE, throwaway: prove the effect-ledger library on two platform hosts,
# basic-cli and basic-webserver, from their installed libhost.a builds.
#
#   ./prove.sh            # ~2 min: waits for an agenda item's minute to pass
#
# Leaves proofs/<UTC>-<pid>/ with transcript.txt, build.txt, user/ (the world
# and its ledgers) and elsewhere/ (ledger copies replayed with the world gone),
# and prints one line per claim: ok or FAIL.
set -u
here=$(cd "$(dirname "$0")" && pwd)
nightlies=$here/roc-nightly
roc_cli=$nightlies/roc_nightly-macos_x86_64-2026-09-26-d6267b4/roc
roc_web=$nightlies/roc_nightly-macos_x86_64-2026-10-09-258ab27/roc
clock=$here/../.claude/skills/verify-tt/scripts/agenda-clock.sh
damage=$here/../.claude/skills/verify-tt/scripts/damage.py
proof=$here/proofs/$(date -u +%Y%m%dT%H%M%SZ)-$$
mkdir -p "$proof/user" "$proof/elsewhere"
log=$proof/transcript.txt
results=()

# tx <command...>: run from the current dir, show and log output and exit.
tx() {
	{
		printf '\n[%s] %s$ ' "$(date -u +%H:%M:%S)" "${PWD#"$proof"/}"
		printf '%q ' "$@"
		printf '\n'
	} >>"$log"
	OUT=$("$@" 2>&1)
	CODE=$?
	printf '%s\n' "$OUT" | tee -a "$log"
	echo "exit $CODE" | tee -a "$log"
}

# check <claim> <exit> <text>: the last tx exited <exit> and printed <text>.
check() {
	if [ "$CODE" = "$2" ] && grep -qF -- "$3" <<<"$OUT"; then
		results+=("ok    $1")
	else
		results+=("FAIL  $1 (exit $CODE, wanted $2 and \"$3\")")
	fi
}

# The pinned binary of a ledger's first instance. Any instance's binary can
# replay or dump the whole ledger.
exe_of() {
	ls -d "$1"/*/exe | head -1
}

# The id of the request whose root span matches <python predicate on name, outcome>.
request_id() {
	python3 - "$1" "$2" <<-'EOF'
	import base64, glob, json, sys
	for line in (l for f in sorted(glob.glob(sys.argv[1] + "/*/spans.jsonl")) for l in open(f)):
	    span = json.loads(line)["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
	    attrs = {a["key"]: a["value"] for a in span["attributes"]}
	    if attrs["ledger.seq"]["intValue"] != "0":
	        continue
	    name, outcome = span["name"], json.loads(base64.b64decode(attrs["ledger.response"]["stringValue"]))
	    if eval(sys.argv[2]):
	        print(span["traceId"]); break
	EOF
}

# serve [ledger]: start agenda-server in the background, recording into
# [ledger] if given, and wait until it listens. Sets SUP.
serve() {
	: >server.log
	if [ -n "${1:-}" ]; then
		env ROC_LEDGER="record:$1" ./agenda-server >>server.log 2>&1 &
	else
		./agenda-server >>server.log 2>&1 &
	fi
	SUP=$!
	for _ in $(seq 100); do grep -q Listening server.log && return; sleep 0.05; done
}

# stopped <label>: wait for the server, then show and log its output and exit
# status, setting OUT and CODE as tx does.
stopped() {
	wait "$SUP"
	CODE=$?
	OUT=$(cat server.log)
	{ echo; echo "[server: $1] output"; echo "$OUT"; echo "server exit $CODE"; } >>"$log"
	printf '%s\nserver exit %s\n' "$OUT" "$CODE"
}

{
	echo "proof          $proof"
	echo "effect-ledger  $(cd "$here/effect-ledger" && shasum -a 256 src/*.rs | shasum -a 256 | cut -c1-16) (src)"
	for p in basic-cli basic-webserver; do
		echo "$p $(git -C "$here/$p" rev-parse --short HEAD) on $(git -C "$here/$p" branch --show-current), uncommitted:"
		git -C "$here/$p" status --short | sed 's/^/    /'
		echo "    host $(shasum -a 256 "$here/$p/platform/targets/x64mac/libhost.a" | cut -c1-16)"
	done
	cd "$here/examples" &&
		"$roc_cli" build agenda.roc --output="$proof/user/agenda" &&
		"$roc_cli" build two_questions.roc --output="$proof/user/two-questions" &&
		"$roc_web" build agenda-server.roc --output="$proof/user/agenda-server"
} >"$proof/build.txt" 2>&1 || { cat "$proof/build.txt"; exit 1; }

cd "$proof/user"
printf '00:01 Early thing\n23:59 Late thing\n' >fine.txt
cp fine.txt tasks.txt

echo "== basic-webserver: one process, many requests"
serve ledgers/web-fine.ledger
tx curl -s -w ' [%{http_code}]\n' http://127.0.0.1:8137/agenda
tx curl -s -w ' [%{http_code}]\n' http://127.0.0.1:8137/nope
curl -s -o slow1.out http://127.0.0.1:8137/slow & s1=$!
curl -s -o slow2.out http://127.0.0.1:8137/slow & s2=$!
curl -s -o slow3.out http://127.0.0.1:8137/slow & s3=$!
wait $s1 $s2 $s3
kill -TERM "$SUP"
stopped web-fine
check "webserver records every request of one process" 143 "recorded 7 request(s)"
tx python3 -c "
import glob, json
for line in open(glob.glob('ledgers/web-fine.ledger/*/spans.jsonl')[0]):
    s = json.loads(line)['resourceSpans'][0]['scopeSpans'][0]['spans'][0]
    seq = next(a['value']['intValue'] for a in s['attributes'] if a['key'] == 'ledger.seq')
    print(s['traceId'][:8], seq, s['name'])"
check "concurrent requests interleave in the file, each with its own sequence" 0 "hosted_sleep_millis"

echo "== basic-cli: interactive request killed by Ctrl-C"
tx python3 "$here/drive.py" "wait:Name?,send:Ann,wait:Age?,ctrlc" -- env ROC_LEDGER=record:ledgers/cli-ctrlc.ledger ./two-questions
check "cli records a Ctrl-C as the outcome" 0 "killed by SIGINT during seq 4 (hosted_stdin_line)"

echo "== the agenda bug, on both platforms, in the same minute"
tx "$clock" due-now tasks.txt
tx env ROC_LEDGER=record:ledgers/cli-crash.ledger ./agenda tasks.txt
check "cli records the crash" 1 "crashed: Integer subtraction overflowed"
serve ledgers/web-crash.ledger
curl -s -o slow4.out http://127.0.0.1:8137/slow & s4=$!
curl -s -o slow5.out http://127.0.0.1:8137/slow & s5=$!
sleep 0.1
tx curl -s -w ' [%{http_code}]\n' http://127.0.0.1:8137/agenda
wait $s4 $s5
stopped web-crash
check "webserver records the crashed request" 1 "crashed: Integer subtraction overflowed"
check "webserver records requests in flight when the process died" 1 "process exited 1 during seq 2 (hosted_sleep_millis)"

echo "== the crashed server restarts into the same ledger"
serve ledgers/web-crash.ledger
tx curl -s -w ' [%{http_code}]\n' http://127.0.0.1:8137/slow
kill -TERM "$SUP"
stopped web-restart
check "a restart records a second instance into the same ledger" 143 "recorded 3 request(s)"

echo "== control: the same tasks.txt after its minute"
tx "$clock" after tasks.txt
tx ./agenda tasks.txt
check "cli control does not crash" 0 "Submit essay draft  done"
serve ""
tx curl -s -w ' [%{http_code}]\n' http://127.0.0.1:8137/agenda
check "webserver control does not crash" 0 "Submit essay draft  done"
kill -TERM "$SUP"
stopped control >/dev/null

echo "== take the world away: replay and dump the ledger copies"
cp -R ledgers/*.ledger "$proof/elsewhere/"
rm -f tasks.txt
cd "$proof/elsewhere"
tx ls -A
tx env -i ROC_LEDGER=replay:cli-crash.ledger "$(exe_of cli-crash.ledger)"
check "cli crash reproduces" 0 "reproduced main!"
tx env -i ROC_LEDGER=replay:cli-ctrlc.ledger "$(exe_of cli-ctrlc.ledger)"
check "cli Ctrl-C reproduces" 0 "killed by SIGINT during seq 4"
tx env -i ROC_LEDGER=replay:web-fine.ledger "$(exe_of web-fine.ledger)"
check "every webserver request reproduces" 0 "reproduced shutdown!"
tx env -i ROC_LEDGER=replay:web-crash.ledger "$(exe_of web-crash.ledger)"
check "the crashed webserver request reproduces" 0 "crashed: Integer subtraction overflowed after 3 crossings"
check "a request that died with the process reproduces" 0 "process exited 1 during seq 2 (hosted_sleep_millis) after 1 crossings"
check "replay covers the restarted instance" 0 "reproduced shutdown!"
crashed=$(request_id web-crash.ledger "name == 'respond!' and 'crashed' in outcome")
tx env -i ROC_LEDGER=replay:web-crash.ledger ROC_LEDGER_REQUEST="${crashed:0:8}" "$(exe_of web-crash.ledger)"
check "one webserver request replays alone, after init!" 0 "reproduced respond! ${crashed:0:8}"
tx env -i ROC_LEDGER=dump:web-crash.ledger "$(exe_of web-crash.ledger)"
check "dump shows each webserver request" 0 "respond!(GET /agenda, 3 headers)"
check "dump merges both instances" 0 "instances  2"
check "dump shows the effect a request died in" 0 "hosted_sleep_millis(300) -> no response: process exited 1"
tx python3 -c "
import sys
rows = sys.argv[1].splitlines()
first = lambda text: next(i for i, r in enumerate(rows) if text in r)
restart = max(i for i, r in enumerate(rows) if 'start  agenda-server' in r)
order = [first('crashed: Integer'), first('process exited 1 during seq 2'), first('exit  process exited 1'), restart]
print('crash, then the request it took down, then the exit, then the restart' if order == sorted(order) else 'out of order: %s' % order)" "$OUT"
check "dump reads in causal order across the restart" 0 "crash, then the request it took down, then the exit, then the restart"
tx env -i ROC_LEDGER=dump:web-crash.ledger ROC_LEDGER_REQUEST="${crashed:0:8}" "$(exe_of web-crash.ledger)"
check "dump narrows to one request" 0 "requests   1 of 7, matching ${crashed:0:8}"
tx env -i ROC_LEDGER=dump:cli-crash.ledger "$(exe_of cli-crash.ledger)"
check "dump shows the cli request" 0 'main!(["./agenda", "tasks.txt"])'

echo "== damaged ledgers"
# damage.py works on one instance: damage <kind> <ledger> <new ledger>.
damage_first() {
	local i
	i=$(ls -d "$2"/*/ | head -1)
	python3 "$damage" "$1" "${i%/}" "$3/$(basename "$i")"
}
tx damage_first tamper cli-crash.ledger cli-tamper.ledger
tx env -i ROC_LEDGER=replay:cli-tamper.ledger "$(exe_of cli-tamper.ledger)"
check "cli tamper diverges" 70 "Mismatch at seq 3 (hosted_stdout_line)"
tx damage_first no-root cli-crash.ledger cli-noroot.ledger
tx env -i ROC_LEDGER=replay:cli-noroot.ledger "$(exe_of cli-noroot.ledger)"
check "cli ledger without a root is refused" 65 "has no root span"
agenda=$(request_id web-fine.ledger "name == 'respond!' and 'Early thing' in __import__('base64').b64decode(outcome['returned']).decode('latin1')")
tx damage_first tamper web-fine.ledger web-tamper.ledger
tx env -i ROC_LEDGER=replay:web-tamper.ledger ROC_LEDGER_REQUEST="${agenda:0:8}" "$(exe_of web-tamper.ledger)"
check "a damaged init! fails the respond! that needs it" 70 "while replaying init!"
cp -R web-fine.ledger web-file.ledger
tx python3 - web-file.ledger "$agenda" <<-'EOF'
	import base64, glob, json, sys
	path, trace = glob.glob(sys.argv[1] + "/*/spans.jsonl")[0], sys.argv[2]
	lines = open(path).read().splitlines()
	for i, line in enumerate(lines):
	    v = json.loads(line)
	    span = v["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
	    if span["traceId"] == trace and span["name"] == "hosted_file_read_utf8":
	        for a in span["attributes"]:
	            if a["key"] == "ledger.response":
	                old = base64.b64decode(a["value"]["stringValue"])
	                a["value"]["stringValue"] = base64.b64encode(old.replace(b"Late thing", b"Later thing")).decode()
	        lines[i] = json.dumps(v)
	        print("changed the file the /agenda request read")
	open(path, "w").write("\n".join(lines) + "\n")
	EOF
tx env -i ROC_LEDGER=replay:web-file.ledger ROC_LEDGER_REQUEST="${agenda:0:8}" "$(exe_of web-file.ledger)"
check "a changed effect response changes the HTTP response: divergence" 70 "Mismatch at seq 0 (respond!)"
restarted=$(request_id web-crash.ledger "name == 'respond!' and 'returned' in outcome")
tx damage_first tamper web-crash.ledger web-scope.ledger
restarted_instance=$(ls -d web-crash.ledger/*/ | tail -1)
cp -R "${restarted_instance%/}" web-scope.ledger/
tx env -i ROC_LEDGER=replay:web-scope.ledger ROC_LEDGER_REQUEST="${restarted:0:8}" "$(exe_of web-scope.ledger)"
check "a request replays after its own instance's init!, not another's" 0 "reproduced respond! ${restarted:0:8}"
tx env -i ROC_LEDGER=replay:web-scope.ledger ROC_LEDGER_REQUEST="${crashed:0:8}" "$(exe_of web-scope.ledger)"
check "the damaged init! fails only its own instance's requests" 70 "while replaying init!"

echo
echo "== proof: $proof"
printf '%s\n' "${results[@]}" | tee "$proof/results.txt"
! grep -q '^FAIL' "$proof/results.txt"
