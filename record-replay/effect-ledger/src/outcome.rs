//! How a request ended. Stored as the response of its root span.

use serde_json::{json, Value};

use crate::otlp::{b64, unb64};
use crate::Entry;

/// How a process went that didn't return: replay makes the child go the same way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Death {
    Signal(i32),
    Exit(i32),
}

impl Death {
    pub fn describe(self) -> String {
        match self {
            Death::Signal(s) => format!("killed by {}", sig_name(s)),
            Death::Exit(c) => format!("process exited {c}"),
        }
    }

    pub fn to_json(self) -> Value {
        match self {
            Death::Signal(s) => json!({"signal": s}),
            Death::Exit(c) => json!({"exited": c}),
        }
    }

    pub fn from_json(v: &Value) -> Option<Death> {
        match (v.get("signal"), v.get("exited")) {
            (Some(s), _) => Some(Death::Signal(s.as_i64()? as i32)),
            (_, Some(c)) => Some(Death::Exit(c.as_i64()? as i32)),
            _ => None,
        }
    }

    pub fn carry_out(self) -> ! {
        match self {
            Death::Signal(sig) => crate::supervisor::die_by_signal(sig),
            Death::Exit(code) => unsafe { libc::_exit(code) },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InFlight {
    pub seq: u64,
    pub effect: String,
    pub request: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The entry returned this, encoded by the platform.
    Returned(Vec<u8>),
    Crashed(String),
    /// The process died under the request: a signal, or an exit from host code
    /// or from another request's crash.
    Died { death: Death, in_flight: Option<InFlight> },
}

impl Outcome {
    pub fn to_json(&self) -> Value {
        match self {
            Outcome::Returned(r) => json!({"returned": b64(r)}),
            Outcome::Crashed(m) => json!({"crashed": m}),
            Outcome::Died { death, in_flight } => {
                let mut v = death.to_json();
                v["in_flight"] = json!(in_flight.as_ref().map(|f| json!({"seq": f.seq, "effect": f.effect, "request": b64(&f.request)})));
                v
            }
        }
    }

    pub fn from_json(bytes: &[u8]) -> Option<Outcome> {
        let v: Value = serde_json::from_slice(bytes).ok()?;
        if let Some(r) = v.get("returned") {
            return Some(Outcome::Returned(unb64(r.as_str()?)?));
        }
        if let Some(m) = v.get("crashed") {
            return Some(Outcome::Crashed(m.as_str()?.to_string()));
        }
        let death = Death::from_json(&v)?;
        let in_flight = match v.get("in_flight").filter(|f| !f.is_null()) {
            None => None,
            Some(f) => Some(InFlight {
                seq: f["seq"].as_u64()?,
                effect: f["effect"].as_str()?.to_string(),
                request: unb64(f["request"].as_str()?)?,
            }),
        };
        Some(Outcome::Died { death, in_flight })
    }

    pub fn describe(&self, entry: Option<&Entry>) -> String {
        match self {
            Outcome::Returned(r) => match entry {
                Some(e) => format!("returned {}", (e.show_response)(r)),
                None => format!("returned <{} bytes>", r.len()),
            },
            Outcome::Crashed(m) => format!("crashed: {m}"),
            Outcome::Died { death, in_flight } => {
                let how = death.describe();
                match in_flight {
                    Some(f) => format!("{how} during seq {} ({})", f.seq, f.effect),
                    None => format!("{how} between crossings"),
                }
            }
        }
    }
}

pub fn sig_name(sig: i32) -> String {
    match sig {
        libc::SIGINT => "SIGINT".into(),
        libc::SIGTERM => "SIGTERM".into(),
        libc::SIGKILL => "SIGKILL".into(),
        libc::SIGSEGV => "SIGSEGV".into(),
        libc::SIGABRT => "SIGABRT".into(),
        libc::SIGQUIT => "SIGQUIT".into(),
        libc::SIGHUP => "SIGHUP".into(),
        n => format!("signal {n}"),
    }
}
