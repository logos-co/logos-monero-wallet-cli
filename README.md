# monero_wallet_cli

The headless Monero wallet — what `monero_wallet_ui` is in Basecamp, for a `logosctl` daemon that
has no window.

A send is built for review (`prepare_send`), the engine signs it at build, and only a configured
**approver** may broadcast it. `logosctl call monero_wallet_backend confirm_send …` is refused on
purpose: the CLI is the host anchor, not a named module. `monero_wallet_cli` is a named module that
holds the role, so the same operator can build, read the preview and broadcast — or withdraw. The
review governs **broadcast**; nothing here is offline signing.

Unlike `evm_signer_cli` there is no separate on-duty approver: Monero's password is a
once-per-session unlock held by the keys surface, the engine signs at build, and the surface that
asks is the surface that confirms. The `prompt` event exists so a preview built by some *other*
module is still seen; it is a convenience, not the centrepiece.

## A session

```bash
logosctl call monero_wallet_backend configure '{"approvers":["monero_wallet_ui","monero_wallet_cli"],"custodians":["monero_keys_ui","monero_keys_cli"]}'
logosctl module load monero_wallet_cli
```

```bash
logosctl call monero_wallet_cli prepare_send '{"address":"5B…","amountXmr":"0.001"}'   # → {"ok":true,"requestId":"s1"}
logosctl call monero_wallet_cli show s1            # destination, amount, fee, total, and the two commands
logosctl call monero_wallet_cli confirm s1     # broadcast
logosctl call monero_wallet_cli cancel s1      # or withdraw; a preview older than 120 s expires unsent
```

`status` → `{ok, held, identity, approvers, custodians, awaiting, hint}`; `list`, `show <id>`,
`send_status <id>`, `wallet_status`, `balances`, `receive_info`, `history` are relayed reads.
