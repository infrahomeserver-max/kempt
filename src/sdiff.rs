//! Structural JSON diff: compares parsed values by path, so key order and
//! formatting never show up as differences.

use crate::diff::{Kind, diff_hashes};
use serde_json::Value;
use std::hash::Hasher;

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Added(String, Value),
    Removed(String, Value),
    Changed(String, Value, Value),
    /// same path, different JSON type (e.g. "5" -> 5)
    TypeChanged(String, Value, Value),
}

impl Change {
    pub fn path(&self) -> &str {
        match self {
            Change::Added(p, _) | Change::Removed(p, _) | Change::Changed(p, _, _) | Change::TypeChanged(p, _, _) => p,
        }
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn key_path(base: &str, k: &str) -> String {
    let simple = !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if simple { format!("{base}.{k}") } else { format!("{base}[{}]", serde_json::to_string(k).unwrap()) }
}

pub fn diff(a: &Value, b: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    walk("$", a, b, &mut out);
    out
}

fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<Change>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            // serde_json maps are ordered by key (BTreeMap), so merge-walk
            let mut xi = x.iter().peekable();
            let mut yi = y.iter().peekable();
            loop {
                match (xi.peek(), yi.peek()) {
                    (None, None) => break,
                    (Some(_), None) => {
                        let (k, v) = xi.next().unwrap();
                        out.push(Change::Removed(key_path(path, k), v.clone()));
                    }
                    (None, Some(_)) => {
                        let (k, v) = yi.next().unwrap();
                        out.push(Change::Added(key_path(path, k), v.clone()));
                    }
                    (Some((kx, _)), Some((ky, _))) => match kx.cmp(ky) {
                        std::cmp::Ordering::Less => {
                            let (k, v) = xi.next().unwrap();
                            out.push(Change::Removed(key_path(path, k), v.clone()));
                        }
                        std::cmp::Ordering::Greater => {
                            let (k, v) = yi.next().unwrap();
                            out.push(Change::Added(key_path(path, k), v.clone()));
                        }
                        std::cmp::Ordering::Equal => {
                            let (k, va) = xi.next().unwrap();
                            let (_, vb) = yi.next().unwrap();
                            walk(&key_path(path, k), va, vb, out);
                        }
                    },
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            // align elements by content (so one insert/delete is ONE change, not a shift of everything)
            let hx: Vec<u64> = x.iter().map(hash_value).collect();
            let hy: Vec<u64> = y.iter().map(hash_value).collect();
            for op in diff_hashes(&hx, &hy, None).ops {
                match op.kind {
                    Kind::Equal => {}
                    Kind::Delete => {
                        for k in 0..op.old_len {
                            out.push(Change::Removed(format!("{path}[{}]", op.old + k), x[op.old + k].clone()));
                        }
                    }
                    Kind::Insert => {
                        for k in 0..op.new_len {
                            out.push(Change::Added(format!("{path}[{}]", op.new + k), y[op.new + k].clone()));
                        }
                    }
                    Kind::Replace => {
                        let m = op.old_len.min(op.new_len);
                        for k in 0..m {
                            walk(&format!("{path}[{}]", op.new + k), &x[op.old + k], &y[op.new + k], out);
                        }
                        for k in m..op.old_len {
                            out.push(Change::Removed(format!("{path}[{}]", op.old + k), x[op.old + k].clone()));
                        }
                        for k in m..op.new_len {
                            out.push(Change::Added(format!("{path}[{}]", op.new + k), y[op.new + k].clone()));
                        }
                    }
                }
            }
        }
        _ => {
            if a == b {
                return;
            }
            if type_name(a) != type_name(b) {
                out.push(Change::TypeChanged(path.to_string(), a.clone(), b.clone()));
            } else {
                out.push(Change::Changed(path.to_string(), a.clone(), b.clone()));
            }
        }
    }
}

/// Order-independent-of-layout content hash (objects are key-sorted by serde_json).
fn hash_value(v: &Value) -> u64 {
    use std::hash::BuildHasher;
    let rs = ahash::RandomState::with_seeds(11, 22, 33, 44);
    let mut h = rs.build_hasher();
    feed(v, &mut h);
    h.finish()
}

fn feed<H: Hasher>(v: &Value, h: &mut H) {
    match v {
        Value::Null => h.write_u8(0),
        Value::Bool(b) => {
            h.write_u8(1);
            h.write_u8(*b as u8)
        }
        Value::Number(n) => {
            h.write_u8(2);
            h.write(n.to_string().as_bytes())
        }
        Value::String(s) => {
            h.write_u8(3);
            h.write_usize(s.len());
            h.write(s.as_bytes())
        }
        Value::Array(a) => {
            h.write_u8(4);
            h.write_usize(a.len());
            for x in a {
                feed(x, h)
            }
        }
        Value::Object(m) => {
            h.write_u8(5);
            h.write_usize(m.len());
            for (k, x) in m {
                h.write_usize(k.len());
                h.write(k.as_bytes());
                feed(x, h)
            }
        }
    }
}

pub fn brief(v: &Value, max: usize) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    if s.chars().count() > max {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_ignored() {
        let a = json!({"a":1,"b":{"x":[1,2],"y":"s"}});
        let b: Value = serde_json::from_str(r#"{"b":{"y":"s","x":[1,2]},"a":1}"#).unwrap();
        assert!(diff(&a, &b).is_empty());
    }

    #[test]
    fn detects_all_kinds() {
        let a = json!({"a":1,"b":"5","c":[1,2,3],"d":true,"gone":1});
        let b = json!({"a":2,"b":5,"c":[1,9],"d":true,"new":{"k":1}});
        let d = diff(&a, &b);
        assert!(d.contains(&Change::Changed("$.a".into(), json!(1), json!(2))));
        assert!(d.contains(&Change::TypeChanged("$.b".into(), json!("5"), json!(5))));
        assert!(d.contains(&Change::Changed("$.c[1]".into(), json!(2), json!(9))));
        assert!(d.contains(&Change::Removed("$.c[2]".into(), json!(3))));
        assert!(d.contains(&Change::Removed("$.gone".into(), json!(1))));
        assert!(d.contains(&Change::Added("$.new".into(), json!({"k":1}))));
        assert_eq!(d.len(), 6);
    }

    #[test]
    fn array_insert_and_delete_are_single_changes() {
        // deleting from the middle must not shift-report every later element
        let a: Vec<Value> = (0..1000).map(|i| json!({"id": i, "v": i * 2})).collect();
        let mut b = a.clone();
        b.remove(400);
        b.insert(10, json!({"id": -1, "v": 0}));
        b[700] = json!({"id": 699, "v": 12345});
        let d = diff(&json!({"data": a}), &json!({"data": b}));
        assert!(d.len() <= 4, "expected few changes, got {}: {:?}", d.len(), &d[..d.len().min(5)]);
        assert!(d.iter().any(|c| matches!(c, Change::Added(p, _) if p == "$.data[10]")));
        assert!(d.iter().any(|c| matches!(c, Change::Removed(p, v) if p == "$.data[400]" && v["id"] == 400)));
        assert!(d.iter().any(|c| matches!(c, Change::Changed(p, _, _) if p.starts_with("$.data[700]"))));
    }

    #[test]
    fn odd_keys_quoted() {
        let a = json!({"a b": 1});
        let b = json!({"a b": 2});
        assert_eq!(diff(&a, &b)[0].path(), "$[\"a b\"]");
    }
}
