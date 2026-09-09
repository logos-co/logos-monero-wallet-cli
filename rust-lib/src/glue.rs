//! Logos glue for `monero_wallet_cli`: the headless approver of Monero broadcasts.
//!
//! A wallet (or this module) asks the backend to BUILD a send; the engine signs it at build; and
//! only a configured approver may broadcast. This module holds that role, shows every previewed
//! send over the event plane (`logosctl watch monero_wallet_cli --event prompt`), and takes the
//! decision over method calls. `concurrency: "multi"`; a poll thread reconciles the backend's
//! send list with what has been shown.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::relay::{configure_hint, holds, translate_refusal};
use crate::render::render;

const ME: &str = "monero_wallet_cli";
const ROLE: &str = "approvers";
const POLL: Duration = Duration::from_millis(1000);

pub trait MoneroWalletCliModule: Send + Sync + 'static {
    /// `{ ok, held, identity, approvers, custodians, awaiting: [requestId], hint }`.
    fn status(&self) -> String;
    /// Build a send for review: `{ address, amountXmr | amount, priority?, accountIndex? }` as one
    /// quoted document. `{ ok, requestId }`; the preview arrives as a `prompt` event.
    fn prepare_send(&self, send_json: String) -> String;
    /// The block a human reads for one previewed send, plus the backend's status.
    fn show(&self, request_id: String) -> String;
    /// APPROVER: broadcast the previewed send. Governs BROADCAST — the engine signed at build.
    fn confirm(&self, request_id: String) -> String;
    /// Withdraw an unbroadcast send.
    fn cancel(&self, request_id: String) -> String;
    /// `{ ok, sends: [{ requestId, state }] }`.
    fn list(&self) -> String;

    // Reads relayed so one module covers a headless session.
    fn send_status(&self, request_id: String) -> String;
    fn wallet_status(&self) -> String;
    fn balances(&self, account_index: i64) -> String;
    fn receive_info(&self, account_index: i64) -> String;
    fn history(&self) -> String;
    fn address_valid(&self, address: String) -> bool;
    fn format_xmr(&self, atomic: String) -> String;
    fn parse_xmr(&self, xmr: String) -> String;
    fn caller_identity(&self) -> String;

    fn on_context_ready(&self, _ctx: &RustModuleContext) {}
}

pub trait MoneroWalletCliModuleEvents {
    /// A send reached `previewed`. `text` is the whole block a human reads.
    fn prompt(&self, request_id: String, text: String);
    /// `sent` | `failed` | `cancelled` — the request left the awaiting set.
    fn settled(&self, request_id: String, state: String);
    fn queue_changed(&self, count: i64);
}

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/provider_gen.rs"));

#[derive(Default)]
struct State {
    shown: BTreeSet<String>,
    awaiting: BTreeSet<String>,
}

#[derive(Default)]
struct MoneroWalletCliModuleImpl {
    state: Arc<Mutex<State>>,
    started: std::sync::atomic::AtomicBool,
}

fn err(msg: impl Into<String>) -> String { json!({ "ok": false, "error": msg.into() }).to_string() }

fn identity() -> Value {
    modules().monero_wallet_backend.caller_identity().ok()
        .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
}

fn hint() -> String { configure_hint(&identity(), ME, ROLE) }

fn relay(reply: Result<String, impl std::fmt::Debug>) -> String {
    match reply { Ok(s) => translate_refusal(&s, ME, "approver", hint), Err(e) => err(format!("wallet backend unreachable: {e:?}")) }
}

fn status_of(id: &str) -> Option<Value> {
    modules().monero_wallet_backend.send_status(id).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Reconcile the backend's send list with what has been shown; called from the poll thread.
fn reconcile(state: &Arc<Mutex<State>>) {
    let Ok(raw) = modules().monero_wallet_backend.list_sends() else { return };
    let v: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let sends = v.get("sends").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut now_awaiting = BTreeSet::new();
    let mut prompts = Vec::new();
    for s in &sends {
        let (Some(id), Some(st)) = (s.get("requestId").and_then(Value::as_str), s.get("state").and_then(Value::as_str)) else { continue };
        if st == "previewed" {
            now_awaiting.insert(id.to_string());
            let fresh = !state.lock().unwrap().shown.contains(id);
            if fresh { if let Some(full) = status_of(id) { prompts.push((id.to_string(), render(&full))); } }
        }
    }
    let (settled, count_changed) = {
        let mut g = state.lock().unwrap();
        let gone: Vec<String> = g.awaiting.difference(&now_awaiting).cloned().collect();
        let changed = g.awaiting != now_awaiting;
        g.awaiting = now_awaiting.clone();
        for (id, _) in &prompts { g.shown.insert(id.clone()); }
        let mut settled = Vec::new();
        for id in gone {
            let st = sends.iter().find(|s| s.get("requestId").and_then(Value::as_str) == Some(&id))
                .and_then(|s| s.get("state").and_then(Value::as_str)).unwrap_or("gone").to_string();
            g.shown.remove(&id);
            settled.push((id, st));
        }
        (settled, changed)
    };
    for (id, text) in prompts { emit_prompt(&id, &text); }
    for (id, st) in settled { emit_settled(&id, &st); }
    if count_changed { emit_queue_changed(now_awaiting.len() as i64); }
}

impl MoneroWalletCliModule for MoneroWalletCliModuleImpl {
    fn on_context_ready(&self, _ctx: &RustModuleContext) {
        if self.started.swap(true, std::sync::atomic::Ordering::SeqCst) { return; }
        let state = Arc::clone(&self.state);
        // Nothing outbound here: a call made inside on_context_ready arrives before the token
        // handshake has settled. The poll thread's first turn is a second away.
        std::thread::spawn(move || loop {
            std::thread::sleep(POLL);
            reconcile(&state);
        });
    }

    fn status(&self) -> String {
        let id = identity();
        let held = holds(&id, ME, ROLE);
        let awaiting: Vec<String> = self.state.lock().unwrap().awaiting.iter().cloned().collect();
        json!({
            "ok": true, "held": held,
            "identity": id.get("identity").cloned().unwrap_or(Value::Null),
            "approvers": id.get("approvers").cloned().unwrap_or(json!([])),
            "custodians": id.get("custodians").cloned().unwrap_or(json!([])),
            "awaiting": awaiting,
            "hint": if held { Value::Null } else { Value::String(configure_hint(&id, ME, ROLE)) },
        }).to_string()
    }

    fn prepare_send(&self, send_json: String) -> String { relay(modules().monero_wallet_backend.prepare_send(&send_json)) }

    fn show(&self, request_id: String) -> String {
        match status_of(request_id.trim()) {
            Some(st) if st.get("ok").and_then(Value::as_bool) == Some(true) => {
                let mut v = st.clone(); v["text"] = json!(render(&st)); v.to_string()
            }
            Some(st) => st.to_string(),
            None => err("wallet backend unreachable"),
        }
    }

    fn confirm(&self, request_id: String) -> String { relay(modules().monero_wallet_backend.confirm_send(request_id.trim())) }
    fn cancel(&self, request_id: String) -> String { relay(modules().monero_wallet_backend.cancel_send(request_id.trim())) }
    fn list(&self) -> String { relay(modules().monero_wallet_backend.list_sends()) }

    fn send_status(&self, request_id: String) -> String { relay(modules().monero_wallet_backend.send_status(request_id.trim())) }
    fn wallet_status(&self) -> String { relay(modules().monero_wallet_backend.wallet_status()) }
    fn balances(&self, account_index: i64) -> String { relay(modules().monero_wallet_backend.balances(account_index)) }
    fn receive_info(&self, account_index: i64) -> String { relay(modules().monero_wallet_backend.receive_info(account_index)) }
    fn history(&self) -> String { relay(modules().monero_wallet_backend.history()) }
    fn address_valid(&self, address: String) -> bool { modules().monero_wallet_backend.address_valid(&address).unwrap_or(false) }
    fn format_xmr(&self, atomic: String) -> String { modules().monero_wallet_backend.format_xmr(&atomic).unwrap_or_default() }
    fn parse_xmr(&self, xmr: String) -> String { modules().monero_wallet_backend.parse_xmr(&xmr).unwrap_or_default() }
    fn caller_identity(&self) -> String { relay(modules().monero_wallet_backend.caller_identity()) }
}

#[no_mangle]
pub extern "Rust" fn logos_module_install() {
    install::<MoneroWalletCliModuleImpl>();
}
