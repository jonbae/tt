## MVP, throwaway: the effect-ledger demo app. Prints today's agenda (UTC).
## Each line of the agenda file is "HH:MM title".
##
## The bug needs both the user's file and the moment they ran it: during an
## item's own minute, `secs_left` underflows and Roc crashes. A minute later the
## item is "done" and nothing crashes, even with the same file.
app [main!] { pf: platform "../basic-cli/platform/main.roc" }

import pf.OsStr
import pf.Path
import pf.Stdout
import pf.Utc

main! : List(OsStr) => Try({}, _)
main! = |args| {
	file = match args {
		[first, ..] => first
		[] => return Err(MissingAgendaFile)
	}
	agenda = Path.from_os_str(file).read_utf8!()?
	now = Utc.now!()
	now_s = (now // 1_000_000_000) % 86_400
	Stdout.line!("Agenda at ${Utc.to_iso_8601(now)}")?
	for line in Str.split_on(agenda, "\n") {
		if !Str.is_empty(line) {
			Stdout.line!(describe(line, now_s))?
		}
	}
	Ok({})
}

describe : Str, U128 -> Str
describe = |line, now_s| {
	words = Str.split_on(line, " ")
	time = List.first(words) ?? ""
	title = Str.join_with(List.drop_first(words, 1), " ")
	due_min = match Str.split_on(time, ":") {
		[h, m] => (U128.from_str(h) ?? 0) * 60 + (U128.from_str(m) ?? 0)
		_ => 0
	}
	now_min = now_s // 60
	if due_min < now_min {
		"${time}  ${title}  done"
	} else {
		# BUG: compares in minutes but subtracts in seconds, so this underflows
		# during the item's own minute.
		secs_left = due_min * 60 - now_s
		"${time}  ${title}  in ${(secs_left // 60).to_str()} min"
	}
}
