//! Logos-free helpers: argument normalisation and refusal translation.

use serde_json::{json, Value};

/// Drop exactly one trailing newline: `@file` hands over the file verbatim, and a password
/// file written with `echo` ends in one.
pub fn strip_file_newline(s: &str) -> &str {
    s.strip_suffix("\r\n").or_else(|| s.strip_suffix('\n')).unwrap_or(s)
}

/// The exact `configure` that adds `me` to `role` while keeping every name already in force —
/// `configure` is total, so a hint naming only `me` would strip the GUI surfaces.
pub fn configure_hint(identity: &Value, me: &str, role: &str) -> String {
    let list = |key: &str| -> Vec<String> {
        identity.get(key).and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    };
    let mut approvers = list("approvers");
    let mut custodians = list("custodians");
    let target = if role == "approvers" { &mut approvers } else { &mut custodians };
    if !target.iter().any(|n| n == me) { target.push(me.to_string()); }
    let doc = json!({ "approvers": approvers, "custodians": custodians });
    format!("logosctl call monero_wallet_backend configure '{doc}'")
}

pub fn holds(identity: &Value, me: &str, role: &str) -> bool {
    identity.get(role).and_then(Value::as_array)
        .map(|a| a.iter().any(|v| v.as_str() == Some(me)))
        .unwrap_or(false)
}

pub fn not_holder(me: &str, role_word: &str, hint: &str) -> String {
    format!("not authorized: {me} is not a configured {role_word}. Run: {hint}")
}

/// The backend's one opaque refusal, turned into a sentence that names the fix. Everything
/// else passes through untouched.
pub fn translate_refusal(reply: &str, me: &str, role_word: &str, hint: impl FnOnce() -> String) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(reply) else { return reply.to_string() };
    if v.get("ok").and_then(Value::as_bool) == Some(false)
        && v.get("error").and_then(Value::as_str) == Some("not authorized")
    {
        v["error"] = Value::String(not_holder(me, role_word, &hint()));
        return v.to_string();
    }
    reply.to_string()
}

/// Best-effort: overwrite the bytes before the allocation is returned.
pub fn scrub(s: &mut String) {
    unsafe { s.as_mut_vec().fill(0) };
    s.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_file_newline_drops_exactly_one() {
        assert_eq!(strip_file_newline("x\n"), "x");
        assert_eq!(strip_file_newline("x"), "x");
    }

    #[test]
    fn the_hint_is_total_safe() {
        let id = json!({ "approvers": ["monero_wallet_ui"], "custodians": ["monero_keys_ui"] });
        assert_eq!(configure_hint(&id, "monero_wallet_cli", "approvers"),
            r#"logosctl call monero_wallet_backend configure '{"approvers":["monero_wallet_ui","monero_wallet_cli"],"custodians":["monero_keys_ui"]}'"#);
    }

    #[test]
    fn only_the_opaque_refusal_is_translated() {
        let out = translate_refusal(r#"{"ok":false,"error":"not authorized"}"#, "monero_wallet_cli", "approver", || "H".into());
        assert!(out.contains("monero_wallet_cli is not a configured approver. Run: H"));
        let other = r#"{"ok":false,"error":"send is previewed, not committing"}"#;
        assert_eq!(translate_refusal(other, "x", "approver", || panic!()), other);
    }
}
