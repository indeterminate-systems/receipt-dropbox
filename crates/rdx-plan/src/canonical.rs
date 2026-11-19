use serde_json::Value;
use sha2::{Digest, Sha256};

/// Canonical JSON: object keys sorted, no whitespace. Digests in the audit trail are taken over
/// this form, so they don't depend on key order (which `serde_json` may preserve, depending on
/// features other crates turn on).
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// `sha256:<hex>` of a value's canonical JSON.
pub fn digest(value: &Value) -> String {
    let hash = Sha256::digest(canonical_json(value).as_bytes());
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_change_the_digest() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"y":[1,2],"x":null}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"x":null,"y":[1,2]},"b":1}"#).unwrap();
        assert_eq!(digest(&a), digest(&b));
        assert_eq!(canonical_json(&a), r#"{"a":{"x":null,"y":[1,2]},"b":1}"#);
    }

    #[test]
    fn digest_is_sha256_of_canonical_form() {
        // echo -n '{"a":1}' | sha256sum
        assert_eq!(
            digest(&json!({"a": 1})),
            "sha256:015abd7f5cc57a2dd94b7590f04ad8084273905ee33ec5cebeae62276a97f862"
        );
    }
}
