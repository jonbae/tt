## A SQLite todo list served over HTTP, built against the ledger-enabled
## basic-webserver fork so every request can be recorded and replayed.
##
## GET / lists the todos with a form to add one. POST /todos adds one, and
## POST /todos/<id>/toggle and /todos/<id>/delete change one; each redirects
## back to /. The database is todos.db in the working directory.
app [Context, program] {
	http: "https://github.com/roc-lang/http/releases/download/1.0.0/6ZUwqYhCS8PU9Mo6MF7oV82ET2o7KYb57CLKDq4cq4sS.tar.zst",
	pf: platform "../record-replay/basic-webserver/platform/main.roc",
}

import pf.Attribute
import pf.Html
import pf.MultipartFormData
import pf.Path
import pf.Server
import pf.Sqlite
import pf.Stdout
import http.Response

Context : { db : Sqlite.Db }

Todo : { id : I64, task : Str, done : I64 }

program = { init!, respond!, shutdown! }

init! : () => Try({ config : Server.Config, context : Context }, [Exit(I64)])
init! = || {
	db = Sqlite.open!(Sqlite.default_config(Path.utf8("todos.db"))) ? |_| Exit(2)
	Sqlite.execute!({
		db,
		query: "CREATE TABLE IF NOT EXISTS todos (id INTEGER PRIMARY KEY, task TEXT NOT NULL, done INTEGER NOT NULL DEFAULT 0);",
		params: {},
	})
		? |_| Exit(2)
	Stdout.line!("todo-app on http://127.0.0.1:8000") ?? {}
	Ok({
		config: Server.default_config.with_listen({ host: "127.0.0.1", port: 8000 }),
		context: { db },
	})
}

respond! : Server.Request, Context => Try(Server.Outcome, [ServerErr(Str)])
respond! = |req, { db }| {
	path =
		match req.target() {
			Resource({ raw_path, .. }) => raw_path
			_ => ""
		}
	Stdout.line!("${Str.inspect(req.method())} ${path}") ?? {}
	match route!(req, db, Str.split_on(path, "/")) {
		Ok(response) => Ok(Server.respond(response))
		Err(err) => Err(ServerErr(Str.inspect(err)))
	}
}

route! : Server.Request, Sqlite.Db, List(Str) => Try(Response, _)
route! = |req, db, parts|
	match req.method() {
		GET =>
			match parts {
				["", ""] => list!(db)
				_ => Ok(text(404, "not found"))
			}
		POST =>
			match parts {
				["", "todos"] => add!(req, db)
				["", "todos", id, "toggle"] => change!(db, id, "UPDATE todos SET done = 1 - done WHERE id = :id;")
				["", "todos", id, "delete"] => change!(db, id, "DELETE FROM todos WHERE id = :id;")
				_ => Ok(text(404, "not found"))
			}
		_ => Ok(text(405, "method not allowed"))
	}

list! : Sqlite.Db => Try(Response, _)
list! = |db| {
	todos : List(Todo)
	todos =
		Sqlite.query_many!({
			db,
			query: "SELECT id, task, done FROM todos ORDER BY id;",
			params: {},
			limits: Sqlite.default_query_limits,
		})
			? |err| DbErr(Str.inspect(err))
	Ok(html(page(todos)))
}

add! : Server.Request, Sqlite.Db => Try(Response, _)
add! = |req, db| {
	body = req.body().with_limit(16 * 1024).read_all!() ? |err| BodyErr(Str.inspect(err))
	form =
		match MultipartFormData.parse_form_url_encoded(body) {
			Ok(fields) => fields
			Err(_) => return Ok(text(400, "malformed form"))
		}
	task = Str.trim(Dict.get(form, "task") ?? "")
	if Str.is_empty(task) {
		Ok(see_other("/"))
	} else {
		Sqlite.execute!({ db, query: "INSERT INTO todos (task) VALUES (:task);", params: { task } })
			? |err| DbErr(Str.inspect(err))
		Ok(see_other("/"))
	}
}

change! : Sqlite.Db, Str, Str => Try(Response, _)
change! = |db, id_str, query|
	match I64.from_str(id_str) {
		Ok(id) => {
			Sqlite.execute!({ db, query, params: { id } }) ? |err| DbErr(Str.inspect(err))
			Ok(see_other("/"))
		}
		Err(_) => Ok(text(404, "not found"))
	}

page : List(Todo) -> Str
page = |todos| {
	row = |todo| {
		label = if todo.done == 0 Html.text(todo.task) else Html.element("s", [], [Html.text(todo.task)])
		Html.li(
			[],
			[
				button("/todos/${todo.id.to_str()}/toggle", if todo.done == 0 "done" else "undo"),
				button("/todos/${todo.id.to_str()}/delete", "delete"),
				label,
			],
		)
	}
	Html.render(
		Html.html(
			[],
			[
				Html.head([], [Html.title([], [Html.text("todos")])]),
				Html.body(
					[],
					[
						Html.h1([], [Html.text("todos")]),
						Html.form(
							[Attribute.method("post"), Attribute.action("/todos")],
							[
								Html.input([Attribute.name("task"), Attribute.attribute("autofocus", "")]),
								Html.button([], [Html.text("add")]),
							],
						),
						Html.ul([], todos.map(row)),
					],
				),
			],
		),
	)
}

button : Str, Str -> Html.HtmlNode
button = |action, label|
	Html.form(
		[Attribute.method("post"), Attribute.action(action), Attribute.style("display: inline")],
		[Html.button([], [Html.text(label)])],
	)

html : Str -> Response
html = |body|
	Response.from_status(200)
		.with_headers([{ name: "Content-Type", value: "text/html; charset=utf-8" }])
		.with_body(Str.to_utf8(body))

text : U16, Str -> Response
text = |status, body|
	Response.from_status(status)
		.with_headers([{ name: "Content-Type", value: "text/plain; charset=utf-8" }])
		.with_body(Str.to_utf8(body))

see_other : Str -> Response
see_other = |location|
	Response.from_status(303).with_headers([{ name: "Location", value: location }])

shutdown! : Server.ShutdownReason, Context => Try({}, [Exit(I64)])
shutdown! = |_reason, _context| Ok({})
