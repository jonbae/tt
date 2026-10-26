#!/usr/bin/env bash
# Dogfood the effect ledger on todo-app: record one session against a real
# SQLite database, take the world away, and replay it.
#
#   ./prove.sh            # rebuilds ./todo-app first if main.roc or the host is newer
#
# Leaves proofs/<UTC>-<pid>/ with transcript.txt, user/ (the world and its
# ledger) and elsewhere/ (a ledger copy replayed with no database), and prints
# one line per claim: ok or FAIL.
set -u
here=$(cd "$(dirname "$0")" && pwd)
roc=${ROC:-$here/../record-replay/roc-nightly/roc_nightly-macos_x86_64-2026-10-09-258ab27/roc}
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

# refute <claim> <text>: the last tx didn't print <text>.
refute() {
	if grep -qF -- "$2" <<<"$OUT"; then
		results+=("FAIL  $1 (printed \"$2\")")
	else
		results+=("ok    $1")
	fi
}

host=$here/../record-replay/basic-webserver/platform/targets/x64mac/libhost.a
if [ ! -x "$here/todo-app" ] || [ "$here/main.roc" -nt "$here/todo-app" ] || [ "$host" -nt "$here/todo-app" ]; then
	(cd "$here" && "$roc" build main.roc --output=todo-app) || exit 1
fi
cp "$here/todo-app" "$proof/user/"

# request <ledger> <exe> <dump row text>: the id of the first request whose
# dump row holds <text>.
request() {
	env -i ROC_LEDGER=dump:"$1" "$2" | awk -v t="$3" 'index($0, t) {print $3; exit}'
}

# rewrite <ledger> <request> <symbol> <request|response> <old> <new>: in the
# first <symbol> crossing of <request>, replace the string <old> with <new>.
# Like damage.py, it edits a copy on purpose, to show what replay catches.
rewrite() {
	python3 - "$@" <<-'EOF'
	import base64, glob, json, struct, sys
	ledger, request, symbol, which, old, new = sys.argv[1:]
	path = glob.glob(ledger + "/*/spans.jsonl")[0]
	lines = open(path).read().splitlines()
	encoded = lambda s: struct.pack("<I", len(s.encode())) + s.encode()
	for i, line in enumerate(lines):
	    v = json.loads(line)
	    span = v["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
	    if span["traceId"].startswith(request) and span["name"] == symbol:
	        for a in span["attributes"]:
	            if a["key"] != "ledger." + which:
	                continue
	            payload = base64.b64decode(a["value"]["stringValue"])
	            if encoded(old) in payload:
	                a["value"]["stringValue"] = base64.b64encode(payload.replace(encoded(old), encoded(new), 1)).decode()
	                lines[i] = json.dumps(v)
	                open(path, "w").write("\n".join(lines) + "\n")
	                print(f"{symbol} {which} in {request}: {old!r} -> {new!r}")
	                sys.exit(0)
	sys.exit(f"no {symbol} {which} in {request} holds {old!r}")
	EOF
}

# post <path> [form]: one form POST, as the page's buttons send it.
post() {
	tx curl -s -o /dev/null -w '%{http_code} %{redirect_url}\n' -d "${2:-}" "http://127.0.0.1:8000$1"
}

echo "== record a session against a real database"
cd "$proof/user"
env ROC_LEDGER=record:todo.ledger ./todo-app >server.log 2>&1 &
sup=$!
for _ in $(seq 100); do grep -q Listening server.log && break; sleep 0.05; done
post /todos 'task=buy+milk'
post /todos 'task=eggs+%26+ham'
post /todos/1/toggle
post /todos/2/delete
tx curl -s http://127.0.0.1:8000/
page=$OUT
check "the page shows the session's result" 0 "<s>buy milk</s>"
kill -TERM "$sup"
wait "$sup"
CODE=$?
OUT=$(cat server.log)
{ echo; echo "[server] output"; echo "$OUT"; echo "server exit $CODE"; } >>"$log"
printf '%s\nserver exit %s\n' "$OUT" "$CODE"
check "the session records every request" 143 "recorded 7 request(s)"

echo "== take the world away"
cp -R todo.ledger "$proof/elsewhere/"
cd "$proof/elsewhere"
exe=$(ls -d todo.ledger/*/exe | head -1)
tx ls -A
check "elsewhere holds only the ledger" 0 "todo.ledger"

tx env -i ROC_LEDGER=dump:todo.ledger "$exe"
check "dump shows the SQL the session ran" 0 "INSERT INTO todos"
check "dump shows the rows SQLite returned" 0 "hosted_sqlite_next_row"

tx env -i ROC_LEDGER=replay:todo.ledger "$exe"
check "every request reproduces with the world gone" 0 "reproduced shutdown!"
refute "no request diverges" "divergence"
tx ls -A
refute "replay never opens a database" "todos.db"

get=$(request todo.ledger "$exe" "begin  respond!(GET /,")
tx env -i ROC_LEDGER=replay:todo.ledger ROC_LEDGER_REQUEST="$get" "$exe"
check "the last page replays alone, byte for byte" 0 "$(printf '%s' "$page" | head -c 200 | sed 's/"/\\"/g')"

echo "== what replay catches"
cp -R todo.ledger row.ledger
tx rewrite row.ledger "$get" hosted_sqlite_next_row response "buy milk" "buy oat milk"
tx env -i ROC_LEDGER=replay:row.ledger ROC_LEDGER_REQUEST="$get" "$(ls -d row.ledger/*/exe)"
check "a changed SQLite row changes the page: divergence" 70 "Mismatch at seq 0 (respond!)"

toggle=$(request todo.ledger "$exe" "begin  respond!(POST /todos/1/toggle,")
cp -R todo.ledger handle.ledger
tx rewrite handle.ledger "$toggle" hosted_sqlite_prepare request "init!#1" "init!#9"
tx env -i ROC_LEDGER=replay:handle.ledger ROC_LEDGER_REQUEST="$toggle" "$(ls -d handle.ledger/*/exe)"
check "a crossing on another handle diverges" 70 "Mismatch at seq 2 (hosted_sqlite_prepare)"

echo
echo "== proof: $proof"
printf '%s\n' "${results[@]}" | tee -a "$log"
! printf '%s\n' "${results[@]}" | grep -q '^FAIL'
