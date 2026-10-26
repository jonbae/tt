#!/usr/bin/env bash
# Dogfood the effect ledger on todo-app: record one session against a real
# SQLite database, take the world away, and replay it.
#
#   ./prove.sh            # builds ./todo-app first if main.roc is newer
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

if [ ! -x "$here/todo-app" ] || [ "$here/main.roc" -nt "$here/todo-app" ]; then
	(cd "$here" && "$roc" build main.roc --output=todo-app) || exit 1
fi
cp "$here/todo-app" "$proof/user/"

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

get=$(env -i ROC_LEDGER=dump:todo.ledger "$exe" | awk '/begin  respond!\(GET \/,/ {print $3; exit}')
tx env -i ROC_LEDGER=replay:todo.ledger ROC_LEDGER_REQUEST="$get" "$exe"
check "the last page replays alone, byte for byte" 0 "$(printf '%s' "$page" | head -c 200 | sed 's/"/\\"/g')"

echo
echo "== proof: $proof"
printf '%s\n' "${results[@]}" | tee -a "$log"
