//! The block a human reads before deciding. Logos-free and unit-tested.

use serde_json::Value;

fn s<'a>(v: &'a Value, k: &str) -> &'a str { v.get(k).and_then(Value::as_str).unwrap_or("") }

/// `status` is the backend's `send_status` reply for a previewed request.
pub fn render(status: &Value) -> String {
    let id = s(status, "requestId");
    let p = status.get("preview").cloned().unwrap_or(Value::Null);
    let line = "-".repeat(64);
    let mut out = String::new();
    out.push_str(&"=".repeat(64)); out.push('\n');
    out.push_str(&format!("SEND AWAITING BROADCAST  {id}\n"));
    out.push_str(&line); out.push('\n');
    out.push_str("Built and SIGNED by the wallet engine. Confirming BROADCASTS it; nothing\n");
    out.push_str("moves until you do, and a preview older than 120 s expires unsent.\n");
    out.push_str(&line); out.push('\n');
    out.push_str(&format!("  To:     {}\n", s(&p, "destination")));
    out.push_str(&format!("  Amount: {} XMR\n", s(&p, "amountXmr")));
    out.push_str(&format!("  Fee:    {} XMR\n", s(&p, "feeXmr")));
    out.push_str(&format!("  Total:  {} XMR\n", s(&p, "totalXmr")));
    if let Some(n) = p.get("txCount").and_then(Value::as_u64) { if n > 1 { out.push_str(&format!("  Split into {n} transactions\n")); } }
    out.push_str(&line); out.push('\n');
    out.push_str(&format!("confirm:  logosctl call monero_wallet_cli confirm {id}\n"));
    out.push_str(&format!("cancel:   logosctl call monero_wallet_cli cancel {id}\n"));
    out.push_str(&"=".repeat(64)); out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_block_names_the_money_and_the_two_commands() {
        let st = json!({ "requestId": "s1", "state": "previewed", "preview": {
            "destination": "5BU3…", "amountXmr": "0.001000000000", "feeXmr": "0.000030000000", "totalXmr": "0.001030000000", "txCount": 1 } });
        let t = render(&st);
        assert!(t.contains("SEND AWAITING BROADCAST  s1"));
        assert!(t.contains("Total:  0.001030000000 XMR"));
        assert!(t.contains("logosctl call monero_wallet_cli confirm s1"));
        assert!(t.contains("logosctl call monero_wallet_cli cancel s1"));
        assert!(t.contains("SIGNED"), "the block must say the engine already signed");
    }
}
