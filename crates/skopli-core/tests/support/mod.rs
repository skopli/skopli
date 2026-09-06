//! Shared conformance-harness support: the structural comparator and number
//! equality per the structural-parity contract. Used by both the conformance
//! test and the comparator self-test.

use serde_json::Value;

/// Structural equality per the structural-parity contract. Returns `Ok(())` on match,
/// or `Err(path)` describing the first mismatch.
///
/// - Objects: same key set (absent on one side != present on the other, even
///   if the present value is null); key order ignored.
/// - Arrays: same length, element-wise in order.
/// - Numbers: bit-equal f64 after parse (+0 == -0), or both integral & equal.
/// - Strings/bools/null: exact equality.
pub fn structural_eq(actual: &Value, expected: &Value, path: &str) -> Result<(), String> {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            let mut keys: Vec<&String> = a.keys().chain(e.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (a.get(key), e.get(key)) {
                    (Some(av), Some(ev)) => structural_eq(av, ev, &child)?,
                    (Some(av), None) => {
                        return Err(format!(
                            "{child}: present in actual ({av}) but ABSENT in expected"
                        ));
                    }
                    (None, Some(ev)) => {
                        return Err(format!(
                            "{child}: ABSENT in actual but present in expected ({ev})"
                        ));
                    }
                    (None, None) => unreachable!(),
                }
            }
            Ok(())
        }
        (Value::Array(a), Value::Array(e)) => {
            if a.len() != e.len() {
                return Err(format!(
                    "{path}: array length {} != expected {}",
                    a.len(),
                    e.len()
                ));
            }
            for (i, (av, ev)) in a.iter().zip(e.iter()).enumerate() {
                structural_eq(av, ev, &format!("{path}[{i}]"))?;
            }
            Ok(())
        }
        (Value::Number(a), Value::Number(e)) => {
            if numbers_equal(a, e) {
                Ok(())
            } else {
                Err(format!("{path}: number {a} != expected {e}"))
            }
        }
        _ => {
            if actual == expected {
                Ok(())
            } else {
                Err(format!("{path}: {actual} != expected {expected}"))
            }
        }
    }
}

/// Numbers equal iff their f64 values are bit-equal after parse (with +0 == -0),
/// or both integral and equal in value. costUsd bit-exactness rides this.
pub fn numbers_equal(a: &serde_json::Number, e: &serde_json::Number) -> bool {
    match (a.as_f64(), e.as_f64()) {
        (Some(x), Some(y)) => x.to_bits() == y.to_bits() || x == y,
        _ => a == e,
    }
}
