//! Logos glue for `monero_wallet_cli`: the headless Monero wallet — what `monero_wallet_ui` is
//! in Basecamp, for a `logosctl` daemon that has no window.
//!
//! It covers a whole session the way `monero-wallet-cli` does: open or create a wallet, read the
//! balance and an address, build a transfer, review it, broadcast or withdraw it, and read the
//! transfers back. That needs BOTH roles — unlock is custodian, broadcast is approver — and this
//! module holds them under one identity because Monero's password is a once-per-session unlock,
//! not a per-signature credential, so the surface that asks is the surface that confirms.
//!
//! Passwords arrive as `@file` or `str:…`; exactly one trailing newline is stripped and the copy
//! is wiped after the call. This module never logs, emits or stores a secret.
//!
//! `concurrency: "multi"`; a poll thread reconciles the backend's send list with what has been
//! shown, so a preview built by some *other* module still reaches `logosctl watch`.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::relay::{configure_hint, holds, scrub, strip_file_newline, translate_refusal};
use crate::render::render;

const ME: &str = "monero_wallet_cli";
/// Every role a full headless session needs. The hint grants all of them at once.
const ROLES: &[&str] = &["custodians", "approvers"];
const POLL: Duration = Duration::from_millis(1000);

pub trait MoneroWalletCliModule: Send + Sync + 'static {
    /// `{ ok, held, custodian, approver, identity, approvers, custodians, awaiting, hint }`.
    /// `held` is "holds every role this module needs"; `hint` names the exact `configure`.
    fn status(&self) -> String;

    // ---- the wallet: custodian-gated (open/create/restore/password/seed/network) ----
    /// CUSTODIAN. Open a registered wallet on the active network. `{ ok, jobId }`; poll job_status.
    fn open_wallet(&self, name: String, password: String) -> String;
    /// CUSTODIAN. `{ ok, jobId }`. The wallet is registered on the active network and left open.
    fn create_wallet(&self, name: String, password: String, label: String) -> String;
    /// CUSTODIAN. `{ name, password, seed, restoreHeight, seedOffset?, label? }` as one quoted
    /// document or `@file`. A restore height of 0 scans from genesis — hours.
    fn restore_from_seed(&self, params_json: String) -> String;
    /// CUSTODIAN. `{ name, password, address, viewKey, spendKey?, restoreHeight, label? }`.
    /// Omit `spendKey` for a view-only wallet.
    fn restore_from_keys(&self, params_json: String) -> String;
    /// CUSTODIAN. `{ ok, jobId }`.
    fn change_password(&self, old_password: String, new_password: String) -> String;
    /// CUSTODIAN. The 25-word seed, after the backend re-checks the password. Never stored.
    fn reveal_seed(&self, password: String) -> String;
    /// CUSTODIAN. The private view key, after the backend re-checks the password.
    fn reveal_view_key(&self, password: String) -> String;
    /// CUSTODIAN. Refused while a wallet is open or a send is in flight.
    fn set_active_network(&self, network: String) -> String;
    /// Either role. Stores and closes the open wallet. `{ ok, jobId }`.
    fn close_wallet(&self) -> String;

    // ---- spending: build, review, decide ----
    /// Build a send for review: `{ address, amountXmr | amount, priority?, accountIndex? }` as one
    /// quoted document. `{ ok, requestId }`; the preview arrives as a `prompt` event.
    fn prepare_send(&self, send_json: String) -> String;
    /// `transfer <address> <amount>` — prepare_send without writing a JSON document by hand.
    fn transfer(&self, address: String, amount_xmr: String) -> String;
    /// The block a human reads for one previewed send, plus the backend's status.
    fn show(&self, request_id: String) -> String;
    /// APPROVER: broadcast the previewed send. Governs BROADCAST — the engine signed at build.
    fn confirm(&self, request_id: String) -> String;
    /// Withdraw an unbroadcast send.
    fn cancel(&self, request_id: String) -> String;
    /// `{ ok, sends: [{ requestId, state }] }`.
    fn list(&self) -> String;

    // ---- reads, relayed so one module covers a headless session ----
    fn send_status(&self, request_id: String) -> String;
    /// `{ ok, state: queued|running|done|failed, result?, error? }` for a lifecycle job id.
    fn job_status(&self, job_id: String) -> String;
    fn wallet_status(&self) -> String;
    fn list_wallets(&self) -> String;
    fn list_networks(&self) -> String;
    fn balances(&self, account_index: i64) -> String;
    /// The primary address and every subaddress of an account.
    fn receive_info(&self, account_index: i64) -> String;
    /// `address new [<label>]` — derive a fresh subaddress on the account.
    fn address_new(&self, account_index: i64, label: String) -> String;
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

fn hint() -> String { configure_hint(&identity(), ME, ROLES) }

/// Relay a reply, naming the role THIS method needed if the backend refused.
fn relay_as(reply: Result<String, impl std::fmt::Debug>, role_word: &str) -> String {
    match reply {
        Ok(s) => translate_refusal(&s, ME, role_word, hint),
        Err(e) => err(format!("wallet backend unreachable: {e:?}")),
    }
}

fn custodian(reply: Result<String, impl std::fmt::Debug>) -> String { relay_as(reply, "custodian") }
fn approver(reply: Result<String, impl std::fmt::Debug>) -> String { relay_as(reply, "approver") }
/// Ungated reads and requests: a refusal here is never about a role, but translating keeps one path.
fn read(reply: Result<String, impl std::fmt::Debug>) -> String { relay_as(reply, "custodian") }

fn with_secret<T>(secret: &mut String, f: impl FnOnce(&str) -> T) -> T {
    let out = f(strip_file_newline(secret));
    scrub(secret);
    out
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
        let custodian = holds(&id, ME, "custodians");
        let approver = holds(&id, ME, "approvers");
        let awaiting: Vec<String> = self.state.lock().unwrap().awaiting.iter().cloned().collect();
        json!({
            "ok": true,
            "held": custodian && approver,
            "custodian": custodian,
            "approver": approver,
            "identity": id.get("identity").cloned().unwrap_or(Value::Null),
            "approvers": id.get("approvers").cloned().unwrap_or(json!([])),
            "custodians": id.get("custodians").cloned().unwrap_or(json!([])),
            "awaiting": awaiting,
            "hint": if custodian && approver { Value::Null } else { Value::String(configure_hint(&id, ME, ROLES)) },
        }).to_string()
    }

    fn open_wallet(&self, name: String, mut password: String) -> String {
        with_secret(&mut password, |pw| custodian(modules().monero_wallet_backend.open_wallet(&name, pw)))
    }
    fn create_wallet(&self, name: String, mut password: String, label: String) -> String {
        with_secret(&mut password, |pw| custodian(modules().monero_wallet_backend.create_wallet(&name, pw, &label)))
    }
    fn restore_from_seed(&self, mut params_json: String) -> String {
        let out = custodian(modules().monero_wallet_backend.restore_from_seed(strip_file_newline(&params_json)));
        scrub(&mut params_json);
        out
    }
    fn restore_from_keys(&self, mut params_json: String) -> String {
        let out = custodian(modules().monero_wallet_backend.restore_from_keys(strip_file_newline(&params_json)));
        scrub(&mut params_json);
        out
    }
    fn change_password(&self, mut old_password: String, mut new_password: String) -> String {
        with_secret(&mut old_password, |o| with_secret(&mut new_password, |n| custodian(modules().monero_wallet_backend.change_password(o, n))))
    }
    fn reveal_seed(&self, mut password: String) -> String {
        with_secret(&mut password, |pw| custodian(modules().monero_wallet_backend.reveal_seed(pw)))
    }
    fn reveal_view_key(&self, mut password: String) -> String {
        with_secret(&mut password, |pw| custodian(modules().monero_wallet_backend.reveal_view_key(pw)))
    }
    fn set_active_network(&self, network: String) -> String {
        custodian(modules().monero_wallet_backend.set_active_network(&network))
    }
    fn close_wallet(&self) -> String { custodian(modules().monero_wallet_backend.close_wallet()) }

    fn prepare_send(&self, send_json: String) -> String {
        read(modules().monero_wallet_backend.prepare_send(strip_file_newline(&send_json)))
    }

    fn transfer(&self, address: String, amount_xmr: String) -> String {
        let doc = json!({ "address": address.trim(), "amountXmr": amount_xmr.trim() }).to_string();
        read(modules().monero_wallet_backend.prepare_send(&doc))
    }

    fn show(&self, request_id: String) -> String {
        match status_of(request_id.trim()) {
            Some(st) if st.get("ok").and_then(Value::as_bool) == Some(true) => {
                let mut v = st.clone(); v["text"] = json!(render(&st)); v.to_string()
            }
            Some(st) => st.to_string(),
            None => err("wallet backend unreachable"),
        }
    }

    fn confirm(&self, request_id: String) -> String { approver(modules().monero_wallet_backend.confirm_send(request_id.trim())) }
    fn cancel(&self, request_id: String) -> String { read(modules().monero_wallet_backend.cancel_send(request_id.trim())) }
    fn list(&self) -> String { read(modules().monero_wallet_backend.list_sends()) }

    fn send_status(&self, request_id: String) -> String { read(modules().monero_wallet_backend.send_status(request_id.trim())) }
    fn job_status(&self, job_id: String) -> String { read(modules().monero_wallet_backend.job_status(job_id.trim())) }
    fn wallet_status(&self) -> String { read(modules().monero_wallet_backend.wallet_status()) }
    fn list_wallets(&self) -> String { read(modules().monero_wallet_backend.list_wallets()) }
    fn list_networks(&self) -> String { read(modules().monero_wallet_backend.list_networks()) }
    fn balances(&self, account_index: i64) -> String { read(modules().monero_wallet_backend.balances(account_index)) }
    fn receive_info(&self, account_index: i64) -> String { read(modules().monero_wallet_backend.receive_info(account_index)) }
    fn address_new(&self, account_index: i64, label: String) -> String {
        read(modules().monero_wallet_backend.create_subaddress(account_index, &label))
    }
    fn history(&self) -> String { read(modules().monero_wallet_backend.history()) }
    fn address_valid(&self, address: String) -> bool { modules().monero_wallet_backend.address_valid(address.trim()).unwrap_or(false) }
    fn format_xmr(&self, atomic: String) -> String { modules().monero_wallet_backend.format_xmr(atomic.trim()).unwrap_or_default() }
    fn parse_xmr(&self, xmr: String) -> String { modules().monero_wallet_backend.parse_xmr(xmr.trim()).unwrap_or_default() }
    fn caller_identity(&self) -> String { read(modules().monero_wallet_backend.caller_identity()) }
}

#[no_mangle]
pub extern "Rust" fn logos_module_install() {
    install::<MoneroWalletCliModuleImpl>();
}
