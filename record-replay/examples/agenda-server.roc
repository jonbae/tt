## PROTOTYPE, throwaway: MVP 1's agenda, served over HTTP.
##
## GET /agenda reads tasks.txt ("HH:MM title" per line) and the clock. Same
## planted bug: during an item's own minute, `secs_left` underflows and Roc
## crashes, which takes the whole server down. GET /slow sleeps, so concurrent
## requests overlap.
app [Context, program] {
	http: "https://github.com/roc-lang/http/releases/download/1.0.0/6ZUwqYhCS8PU9Mo6MF7oV82ET2o7KYb57CLKDq4cq4sS.tar.zst",
	pf: platform "../basic-webserver/platform/main.roc",
}

import pf.Path
import pf.Server
import pf.Sleep
import pf.Stdout
import pf.UnixTime
import http.Response

Context : { agenda_file : Str }

program = { init!, respond!, shutdown! }

init! : () => Try({ config : Server.Config, context : Context }, [Exit(I64)])
init! = || {
	Stdout.line!("agenda-server on 127.0.0.1:8137") ?? {}
	Ok({
		config: Server.default_config.with_listen({ host: "127.0.0.1", port: 8137 }),
		context: { agenda_file: "tasks.txt" },
	})
}

respond! : Server.Request, Context => Try(Server.Outcome, [ServerErr(Str)])
respond! = |request, context|
	match request.target() {
		Resource({ raw_path: "/agenda", .. }) => {
			agenda = Path.read_utf8!(Path.utf8(context.agenda_file)) ? |err| ServerErr("read ${context.agenda_file}: ${Str.inspect(err)}")
			now_s = I64.to_u64_wrap(UnixTime.now!().seconds_since_epoch()) % 86_400
			Stdout.line!("GET /agenda at ${now_s.to_str()}s") ?? {}
			lines = Str.split_on(agenda, "\n").keep_if(|line| !Str.is_empty(line)).map(|line| describe(line, now_s))
			Ok(text(200, Str.join_with(lines, "\n")))
		}
		Resource({ raw_path: "/slow", .. }) => {
			Stdout.line!("GET /slow begin") ?? {}
			Sleep.millis!(300)
			now_s = I64.to_u64_wrap(UnixTime.now!().seconds_since_epoch()) % 86_400
			Stdout.line!("GET /slow end at ${now_s.to_str()}s") ?? {}
			Ok(text(200, "slow ${now_s.to_str()}"))
		}
		_ => Ok(text(404, "not found"))
	}

shutdown! : Server.ShutdownReason, Context => Try({}, [Exit(I64)])
shutdown! = |_reason, _context| {
	Stdout.line!("agenda-server shutting down") ?? {}
	Ok({})
}

describe : Str, U64 -> Str
describe = |line, now_s| {
	words = Str.split_on(line, " ")
	time = List.first(words) ?? ""
	title = Str.join_with(List.drop_first(words, 1), " ")
	due_min = match Str.split_on(time, ":") {
		[h, m] => (U64.from_str(h) ?? 0) * 60 + (U64.from_str(m) ?? 0)
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

text = |status, body|
	Server.respond(
		Response.from_status(status)
			.with_headers([{ name: "Content-Type", value: "text/plain; charset=utf-8" }])
			.with_body(Str.to_utf8(body)),
	)
