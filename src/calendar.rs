//! Calendar events: validation only. The events themselves live in `calendar.json` (see state.rs) and are drawn by the
//! browser. Every event carries what a later sync needs: a permanent id, a change time (`u`, ms) and, when it was deleted,
//! a tombstone (`x`) that is kept so the deletion can travel to other devices.
//!
//! The data model follows the iCalendar ideas (start/end, all-day, repeat rule, exceptions, reminders) without depending on any
//! outside library or service.

use serde_json::{json, Map, Value};

pub const MAX_EVENTS: usize = 5000;
const MAX_TITLE: usize = 300;
const MAX_TEXT: usize = 5000;
/// 1970-01-01 minus 100 years to year 2200, in ms: anything else is a typo or an attack
const MIN_MS: f64 = -3.2e12;
const MAX_MS: f64 = 7.3e12;

fn num(v: &Value, lo: f64, hi: f64) -> Option<f64> {
    let n = v.as_f64()?;
    (n.is_finite() && n >= lo && n <= hi).then_some(n)
}

fn text(v: &Value, max: usize) -> Option<String> {
    let s = v.as_str()?;
    (s.chars().count() <= max).then(|| s.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect())
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':' | b'|'))
}

fn rule(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    let f = o.get("f")?.as_str()?;
    if !matches!(f, "d" | "w" | "m" | "y") {
        return None;
    }
    let n = num(o.get("n").unwrap_or(&json!(1)), 1.0, 999.0)?.trunc();
    let mut r = Map::new();
    r.insert("f".into(), json!(f));
    r.insert("n".into(), json!(n));
    if let Some(by) = o.get("by").and_then(Value::as_array) {
        let mut days: Vec<i64> = by.iter().filter_map(|d| num(d, 0.0, 6.0).map(|x| x as i64)).collect();
        days.sort_unstable();
        days.dedup();
        if !days.is_empty() {
            r.insert("by".into(), json!(days));
        }
    }
    if let Some(u) = o.get("until").and_then(|u| num(u, MIN_MS, MAX_MS)) {
        r.insert("until".into(), json!(u));
    }
    if let Some(c) = o.get("count").and_then(|c| num(c, 1.0, 5000.0)) {
        r.insert("count".into(), json!(c.trunc()));
    }
    Some(Value::Object(r))
}

fn day_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// One event, reduced to the known fields with the right types. None means the event is rejected.
pub fn clean_event(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    let id = o.get("id")?.as_str()?;
    if !valid_id(id) {
        return None;
    }
    let mut e = Map::new();
    e.insert("id".into(), json!(id));
    e.insert("u".into(), json!(num(o.get("u").unwrap_or(&json!(0)), 0.0, MAX_MS).unwrap_or(0.0)));
    if let Some(h) = o.get("h").and_then(|h| text(h, 40)) {
        e.insert("h".into(), json!(h));
    }
    if o.get("x") == Some(&json!(true)) {
        e.insert("x".into(), json!(true)); // tombstone: only what a sync needs
        return Some(Value::Object(e));
    }
    e.insert("t".into(), json!(text(o.get("t")?, MAX_TITLE)?));
    let s = num(o.get("s")?, MIN_MS, MAX_MS)?;
    e.insert("s".into(), json!(s));
    if let Some(en) = o.get("e").and_then(|n| num(n, MIN_MS, MAX_MS)) {
        e.insert("e".into(), json!(en.max(s)));
    }
    if o.get("ad") == Some(&json!(true)) {
        e.insert("ad".into(), json!(true));
    }
    if let Some(r) = o.get("r").and_then(rule) {
        e.insert("r".into(), r);
    }
    if let Some(ex) = o.get("ex").and_then(Value::as_array) {
        let days: Vec<&str> = ex.iter().filter_map(Value::as_str).filter(|d| day_key(d)).take(500).collect();
        if !days.is_empty() {
            e.insert("ex".into(), json!(days));
        }
    }
    if let Some(rem) = o.get("rem").and_then(Value::as_array) {
        let mut m: Vec<i64> = rem.iter().filter_map(|n| num(n, 0.0, 525_600.0).map(|x| x as i64)).take(5).collect();
        m.sort_unstable();
        m.dedup();
        if !m.is_empty() {
            e.insert("rem".into(), json!(m));
        }
    }
    if let Some(c) = o.get("c").and_then(Value::as_str) {
        if matches!(c, "w" | "p") {
            e.insert("c".into(), json!(c));
        }
    }
    for (k, max) in [("d", MAX_TEXT), ("l", MAX_TITLE)] {
        if let Some(t) = o.get(k).and_then(|t| text(t, max)).filter(|t| !t.is_empty()) {
            e.insert(k.into(), json!(t));
        }
    }
    Some(Value::Object(e))
}

/// All events, or an error message if the list is unusable. Duplicate ids keep the newest change.
pub fn clean_events(v: &Value) -> Result<Vec<Value>, String> {
    let a = v.as_array().ok_or("events must be a list")?;
    if a.len() > MAX_EVENTS {
        return Err(format!("too many events (limit {MAX_EVENTS})"));
    }
    let mut out: Vec<Value> = Vec::with_capacity(a.len());
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for x in a {
        let Some(e) = clean_event(x) else { return Err("an event is invalid (needs an id, a title and a start time)".into()) };
        let id = e["id"].as_str().unwrap_or("").to_string();
        match at.get(&id) {
            Some(&i) => {
                if e["u"].as_f64() >= out[i]["u"].as_f64() {
                    out[i] = e;
                }
            }
            None => {
                at.insert(id, out.len());
                out.push(e);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_known_fields_and_rejects_bad_ones() {
        let e = clean_event(&json!({"id":"a1","t":"Stand-up","s":1.7e12,"e":1.7e12+1800000.0,"r":{"f":"w","n":1,"by":[1,3,1],"until":1.8e12},"rem":[10,0,10],"c":"w","evil":"<script>","ad":false})).unwrap();
        assert_eq!(e["r"]["by"], json!([1, 3]));
        assert_eq!(e["rem"], json!([0, 10]));
        assert!(e.get("evil").is_none() && e.get("ad").is_none());
        assert!(clean_event(&json!({"id":"../x","t":"a","s":1})).is_none());
        assert!(clean_event(&json!({"id":"a","t":"a"})).is_none());
        assert!(clean_event(&json!({"id":"a","t":"a","s":1e20})).is_none());
        assert!(clean_event(&json!({"id":"a","t":"x".repeat(301),"s":1})).is_none());
        let r = clean_event(&json!({"id":"a","t":"a","s":1,"r":{"f":"x"}})).unwrap();
        assert!(r.get("r").is_none());
    }

    #[test]
    fn tombstones_keep_only_what_sync_needs() {
        let e = clean_event(&json!({"id":"a1","x":true,"u":5,"t":"secret","d":"private notes"})).unwrap();
        assert_eq!(e, json!({"id":"a1","u":5.0,"x":true}));
    }

    #[test]
    fn newest_duplicate_wins_and_limits_hold() {
        let v = clean_events(&json!([{"id":"a","t":"old","s":1,"u":1},{"id":"a","t":"new","s":1,"u":2},{"id":"b","t":"b","s":2}])).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0]["t"], "new");
        let big: Vec<Value> = (0..MAX_EVENTS + 1).map(|i| json!({"id":format!("e{i}"),"t":"x","s":1})).collect();
        assert!(clean_events(&Value::Array(big)).is_err());
        assert!(clean_events(&json!({"a":1})).is_err());
    }
}
