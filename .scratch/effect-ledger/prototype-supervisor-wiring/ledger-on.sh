# MVP, throwaway: always-on recording, with no code in the host.
# Source this file, then put `ledgered` in front of an app, or alias the app:
#
#   . ledger-on.sh
#   alias agenda='ledgered agenda'
#
# Each invocation records its own ledger:
#   ${ROC_LEDGER_DIR:-~/.ledgers}/<app>-<UTC time>-<pid>-<random>.ledger
# Recording refuses an existing directory, so every name must be new.
ledgered() {
	local dir="${ROC_LEDGER_DIR:-$HOME/.ledgers}"
	mkdir -p "$dir" && chmod 700 "$dir"
	ROC_LEDGER="record:$dir/$(basename "$1")-$(date -u +%Y%m%dT%H%M%SZ)-$$-$RANDOM.ledger" "$@"
}
