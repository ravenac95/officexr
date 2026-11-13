use std::fs;

use officexr_protocol::{Frame, NetEvent};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

/// JSON numbers compare by value so `7` equals the `7.0` an `f64` field serializes to.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

fn round_trip<T: DeserializeOwned + Serialize>(prefix: &str) -> usize {
    let mut count = 0;
    for entry in fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(prefix)
        {
            continue;
        }
        let expected: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let typed: T =
            serde_json::from_value(expected.clone()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let actual = serde_json::to_value(&typed).unwrap();
        assert!(
            same(&expected, &actual),
            "{path:?}\nexpected {expected}\nactual   {actual}"
        );
        count += 1;
    }
    count
}

#[test]
fn every_net_event_kind_round_trips() {
    assert_eq!(round_trip::<NetEvent>("event-"), 16);
}

#[test]
fn every_frame_type_round_trips() {
    assert_eq!(round_trip::<Frame>("frame-"), 7);
}
