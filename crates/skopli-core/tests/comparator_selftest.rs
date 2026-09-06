//! Proves the structural comparator has teeth: it must reject absent-vs-null in
//! both directions, catch f64 mismatches, accept integral-int/float equality,
//! and catch array-length differences.

use serde_json::json;

mod support;
use support::structural_eq;

#[test]
fn absent_is_not_null() {
    let actual = json!({"a": 1});
    let expected = json!({"a": 1, "b": null});
    assert!(structural_eq(&actual, &expected, "").is_err());
}

#[test]
fn null_is_not_absent() {
    let actual = json!({"a": 1, "b": null});
    let expected = json!({"a": 1});
    assert!(structural_eq(&actual, &expected, "").is_err());
}

#[test]
fn number_mismatch_detected() {
    assert!(structural_eq(&json!(0.0123), &json!(0.0124), "").is_err());
    assert!(structural_eq(&json!(3), &json!(3.0), "").is_ok());
    assert!(structural_eq(&json!(0.0123), &json!(0.0123), "").is_ok());
}

#[test]
fn array_length_detected() {
    assert!(structural_eq(&json!([1, 2]), &json!([1, 2, 3]), "").is_err());
}

#[test]
fn key_order_ignored() {
    let actual = json!({"b": 2, "a": 1});
    let expected = json!({"a": 1, "b": 2});
    assert!(structural_eq(&actual, &expected, "").is_ok());
}
