//! Logical names for handles: boxes a hosted function hands the core that
//! stand for host-side resources, such as a database or a statement.
//!
//! A handle's address differs on every run, and replay never creates the real
//! resource, so a crossing can't record the handle itself. It records the
//! crossing that created it instead: `init!#2` is the handle returned by seq 2
//! of `init!`. The name is the same in a recording and its replay, because a
//! replayed request re-runs only its `after` entries and itself.
//!
//! Requests are independent, so two concurrent `respond!`s may each hold a
//! `respond!#3`. Each only ever names its own, plus what `init!` made.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// What a handle no crossing created is called. A crossing that takes one
/// still records, but the ledger can't tell it apart from another.
pub const UNKNOWN: &str = "?";

fn names() -> MutexGuard<'static, HashMap<usize, String>> {
    static NAMES: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();
    NAMES.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner())
}

/// In the crossing that returned `handle`, named by its address: name it after
/// this crossing, and return the name. Recording calls it with the real
/// handle, and replay with the stand-in it made in its place.
pub fn created(handle: usize) -> String {
    let name = match crate::current_crossing() {
        Some((entry, seq)) => format!("{entry}#{seq}"),
        None => UNKNOWN.to_string(),
    };
    names().insert(handle, name.clone());
    name
}

/// The name of a handle a crossing takes as an argument.
pub fn name(handle: usize) -> String {
    names().get(&handle).cloned().unwrap_or_else(|| UNKNOWN.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Current, CURRENT};

    fn crossing(entry: &'static str, seq: u64) {
        CURRENT.with(|c| c.set(Some(Current { rid: 1, entry, seq })));
    }

    #[test]
    fn a_handle_is_named_by_the_crossing_that_made_it() {
        crossing("init!", 2);
        assert_eq!(created(0x1000), "init!#2");
        crossing("respond!", 5);
        assert_eq!(name(0x1000), "init!#2");
    }

    #[test]
    fn a_reused_address_takes_its_new_name() {
        crossing("respond!", 3);
        created(0x2000);
        crossing("respond!", 7);
        created(0x2000);
        assert_eq!(name(0x2000), "respond!#7");
    }

    #[test]
    fn a_handle_no_crossing_made_is_unknown() {
        assert_eq!(name(0x3000), UNKNOWN);
        CURRENT.with(|c| c.set(None));
        assert_eq!(created(0x4000), UNKNOWN);
    }
}
