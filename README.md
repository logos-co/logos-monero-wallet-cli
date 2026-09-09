# monero_wallet_cli

The headless approver for `monero_wallet_backend` — what `monero_wallet_ui` is in Basecamp, for a
`logosctl` daemon that has no window to show anything in.

A send is built for review (`prepare_send`), the engine signs it at build, and only a configured
**approver** may broadcast it. `logosctl call monero_wallet_backend confirm_send …` is refused on
purpose: the CLI is the host anchor, not a named module. `monero_wallet_cli` is a named module: it
holds the role, shows every previewed send over the event plane, and takes the decision over method
calls. The review governs **broadcast** — nothing here is offline signing.

## A session

```bash
logosctl call monero_wallet_backend configure '{"approvers":["monero_wallet_ui","monero_wallet_cli"],"custodians":["monero_keys_ui","monero_keys_cli"]}'
logosctl module load monero_wallet_cli
```

Terminal 1, for as long as you are on duty:

```bash
logosctl watch monero_wallet_cli --event prompt
```

Terminal 2:

```bash
logosctl call monero_wallet_cli prepare_send '{"address":"5B…","amountXmr":"0.001"}'   # → {"ok":true,"requestId":"s1"}
# … the prompt block appears in terminal 1: destination, amount, fee, total, and the two commands …
logosctl call monero_wallet_cli confirm s1     # broadcast
logosctl call monero_wallet_cli cancel s1      # or withdraw; a preview older than 120 s expires unsent
```

`status` → `{ok, held, identity, approvers, custodians, awaiting, hint}`; `list`, `show <id>`,
`send_status <id>`, `wallet_status`, `balances`, `receive_info`, `history` are relayed reads.
