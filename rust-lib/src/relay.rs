//! Logos-free helpers: argument normalisation and refusal translation.

use serde_json::{json, Value};

/// Drop exactly one trailing newline: `@file` hands over the file verbatim, and a password
/// file written with `echo` ends in one.
pub fn strip_file_newline(s: &str) -> &str {
    s.strip_suffix("\r\n").or_else(|| s.strip_suffix('\n')).unwrap_or(s)
}

/// The exact `configure` that adds `me` to every role in `roles` while keeping every name
/// already in force — `configure` is total, so a hint naming only `me` would strip the GUI.
///
/// This module needs BOTH roles to cover a whole headless session (unlock is custodian,
/// broadcast is approver), so the hint grants both at once and one call fixes any refusal.
/// An operator who wants a box that unlocks at boot but never broadcasts writes the narrower
/// document by hand — the roles are independent sets precisely so that is expressible.
pub fn configure_hint(identity: &Value, me: &str, roles: &[&str]) -> String {
    let list = |key: &str| -> Vec<String> {
        identity.get(key).and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    };
    let mut approvers = list("approvers");
    let mut custodians = list("custodians");
    for role in roles {
        let target = if *role == "approvers" { &mut approvers } else { &mut custodians };
        if !target.iter().any(|n| n == me) { target.push(me.to_string()); }
    }
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

/// The backend's one opaque refusal, turned into a sentence that names the fix. `role_word` is
/// the role THIS method needed, so a refusal says which half is missing even though the hint
/// grants both. Everything else passes through untouched.
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
        assert_eq!(strip_file_newline("x\n\n"), "x\n");
    }

    #[test]
    fn the_hint_grants_both_roles_and_keeps_the_gui() {
        let id = json!({ "approvers": ["monero_wallet_ui"], "custodians": ["monero_wallet_ui"] });
        assert_eq!(configure_hint(&id, "monero_wallet_cli", &["custodians", "approvers"]),
            r#"logosctl call monero_wallet_backend configure '{"approvers":["monero_wallet_ui","monero_wallet_cli"],"custodians":["monero_wallet_ui","monero_wallet_cli"]}'"#);
    }

    #[test]
    fn the_hint_is_idempotent_and_can_grant_one_role() {
        let id = json!({ "approvers": ["monero_wallet_ui", "monero_wallet_cli"], "custodians": ["monero_wallet_ui"] });
        let both = configure_hint(&id, "monero_wallet_cli", &["custodians", "approvers"]);
        assert_eq!(both.matches("monero_wallet_cli").count(), 2, "already-held roles are not duplicated");
        let one = configure_hint(&id, "monero_wallet_cli", &["custodians"]);
        assert!(one.contains(r#""custodians":["monero_wallet_ui","monero_wallet_cli"]"#));
    }

    #[test]
    fn only_the_opaque_refusal_is_translated() {
        let out = translate_refusal(r#"{"ok":false,"error":"not authorized"}"#, "monero_wallet_cli", "custodian", || "H".into());
        assert!(out.contains("monero_wallet_cli is not a configured custodian. Run: H"));
        let other = r#"{"ok":false,"error":"send is previewed, not committing"}"#;
        assert_eq!(translate_refusal(other, "x", "approver", || panic!()), other);
    }

    #[test]
    fn scrub_clears_the_bytes() {
        let mut s = String::from("hunter2");
        scrub(&mut s);
        assert!(s.is_empty());
    }
}
