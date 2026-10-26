//! The ledger file: OTLP JSON Lines, one span per line, one trace per request.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::{json, Value};

use crate::outcome::Outcome;

pub fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

pub fn unb64(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// Nanoseconds since the epoch as ISO 8601 UTC, to the microsecond.
pub fn iso_utc(nanos: u128) -> String {
    let secs = (nanos / 1_000_000_000) as i64;
    let micros = (nanos % 1_000_000_000) / 1000;
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{micros:06}Z", sod / 3600, sod % 3600 / 60, sod % 60)
}

pub fn now_nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
}

pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).unwrap();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn attr_str(k: &str, v: &str) -> Value {
    json!({"key": k, "value": {"stringValue": v}})
}

pub fn attr_int(k: &str, v: i64) -> Value {
    json!({"key": k, "value": {"intValue": v.to_string()}})
}

pub fn attr<'a>(span: &'a Value, key: &str) -> Option<&'a Value> {
    span["attributes"].as_array()?.iter().find(|a| a["key"] == key).map(|a| &a["value"])
}

/// One request's trace. The request id is the traceId.
pub struct Trace {
    pub trace_id: String,
    /// Chosen up front so crossings can name their parent before the root,
    /// which is written last, exists.
    pub root_span_id: String,
}

impl Trace {
    pub fn new() -> Trace {
        Trace { trace_id: random_hex(16), root_span_id: random_hex(8) }
    }

    /// Each span is its own `TracesData` line, resource included.
    fn line(&self, resource: &Value, span: Value) -> String {
        let v = json!({"resourceSpans": [{
            "resource": resource,
            "scopeSpans": [{"scope": {"name": "roc.effect_ledger"}, "spans": [span]}],
        }]});
        let mut s = v.to_string();
        s.push('\n');
        s
    }

    /// `response` is `None` for the crossing a process died in: begun, never answered.
    #[allow(clippy::too_many_arguments)]
    pub fn crossing(&self, resource: &Value, seq: u64, effect: &str, start: u128, end: u128, request: &[u8], response: Option<&[u8]>) -> String {
        let mut attributes = vec![attr_int("ledger.seq", seq as i64), attr_str("ledger.effect", effect), attr_str("ledger.request", &b64(request))];
        let mut span = json!({
            "traceId": self.trace_id,
            "spanId": random_hex(8),
            "parentSpanId": self.root_span_id,
            "name": effect,
            "kind": 3, // CLIENT: the core asks the world
            "startTimeUnixNano": start.to_string(),
            "endTimeUnixNano": end.to_string(),
        });
        match response {
            Some(r) => attributes.push(attr_str("ledger.response", &b64(r))),
            None => span["status"] = json!({"code": 2, "message": "no response"}),
        }
        span["attributes"] = Value::Array(attributes);
        self.line(resource, span)
    }

    /// The host-to-core crossing that is the request. Written last.
    #[allow(clippy::too_many_arguments)]
    pub fn root(&self, resource: &Value, entry: &str, start: u128, end: u128, request: &[u8], outcome: &Outcome, ok: bool, described: &str) -> String {
        self.line(resource, json!({
            "traceId": self.trace_id,
            "spanId": self.root_span_id,
            "name": entry,
            "kind": 2, // SERVER: the host asks the core
            "startTimeUnixNano": start.to_string(),
            "endTimeUnixNano": end.to_string(),
            "attributes": [
                attr_int("ledger.seq", 0),
                attr_str("ledger.effect", entry),
                attr_str("ledger.request", &b64(request)),
                attr_str("ledger.response", &b64(outcome.to_json().to_string().as_bytes())),
            ],
            // Standard OTLP status, so a backend shows a failed request
            // without decoding payloads.
            "status": if ok { json!({"code": 1}) } else { json!({"code": 2, "message": described}) },
        }))
    }
}

/// One span line as read back.
pub struct Span {
    pub trace_id: String,
    pub seq: u64,
    pub effect: String,
    pub start: u128,
    pub end: u128,
    pub request: Vec<u8>,
    /// Empty when not `answered`.
    pub response: Vec<u8>,
    pub answered: bool,
    pub resource: Value,
}

impl Span {
    pub fn parse(line: &str) -> Result<Span, &'static str> {
        let v: Value = serde_json::from_str(line).map_err(|_| "torn")?;
        let resource = v["resourceSpans"][0]["resource"].clone();
        let span = &v["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        let s = |k| attr(span, k).and_then(|v| v["stringValue"].as_str());
        let nanos = |k: &str| span[k].as_str().and_then(|t| t.parse().ok()).unwrap_or(0);
        let parsed = (|| {
            Some(Span {
                trace_id: span["traceId"].as_str()?.to_string(),
                seq: attr(span, "ledger.seq")?["intValue"].as_str()?.parse().ok()?,
                effect: s("ledger.effect")?.to_string(),
                start: nanos("startTimeUnixNano"),
                end: nanos("endTimeUnixNano"),
                request: unb64(s("ledger.request")?)?,
                response: match s("ledger.response") {
                    Some(r) => unb64(r)?,
                    None => Vec::new(),
                },
                answered: s("ledger.response").is_some(),
                resource,
            })
        })();
        parsed.ok_or("not a ledger span")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_utc_formats() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00.000000Z");
        assert_eq!(iso_utc(1_791_400_065_828_387_000), "2026-10-07T19:07:45.828387Z");
        assert_eq!(iso_utc(951_782_400_000_000_000), "2000-02-29T00:00:00.000000Z");
    }
}
