# monero_wallet_cli

The headless Monero wallet — what `monero_wallet_ui` is in Basecamp, for a `logosctl` daemon that
has no window. One module covers a whole session, in the same order `monero-wallet-cli` does:
open or create a wallet, read the balance and an address, transfer, review, broadcast.

`logosctl call monero_wallet_backend open_wallet …` is refused on purpose: the CLI authenticates as
the host anchor, which no role admits. `monero_wallet_cli` is a named module that holds the roles.

## Roles

Two roles gate the backend, and this module needs both:

| role | governs | why |
|---|---|---|
| **custodian** | open · create · restore · change password · reveal seed/view key · switch network | everything that takes or reveals the wallet password or a key |
| **approver** | `confirm` | the one decision that moves money |

They stay separate sets so an operator can grant one without the other — a box that unlocks at boot
but must never broadcast is `custodians` without `approvers`. `status` reports both, and any refusal
prints the exact `configure` that fixes it (`configure` is TOTAL, so the hint restates every name
already in force rather than replacing them).

## A session

```bash
logosctl module load monero_wallet_cli
logosctl call monero_wallet_cli status            # → held:false, plus the exact configure to run
logosctl call monero_wallet_backend configure '{"approvers":["monero_wallet_ui","monero_wallet_cli"],"custodians":["monero_wallet_ui","monero_wallet_cli"]}'
```

```bash
# open a wallet — the password never appears in argv or shell history
umask 077; printf '%s\n' 'wallet password' > /run/user/501/pw
logosctl call monero_wallet_cli set_active_network stagenet
logosctl call monero_wallet_cli create_wallet main @/run/user/501/pw str:Main   # → {"ok":true,"jobId":"b1"}
logosctl call monero_wallet_cli job_status b1                                   # poll until done
logosctl call monero_wallet_cli reveal_seed @/run/user/501/pw                   # 25 words, once, never stored
```

```bash
# read it
logosctl call monero_wallet_cli wallet_status     # state, heights, sync %
logosctl call monero_wallet_cli balances 0        # balance and unlocked balance
logosctl call monero_wallet_cli receive_info 0    # primary address + subaddresses
logosctl call monero_wallet_cli address_new 0 str:donations
logosctl call monero_wallet_cli history
```

```bash
# spend it: build, review, decide
logosctl call monero_wallet_cli transfer 5B… str:0.001     # → {"ok":true,"requestId":"s1"}
logosctl call monero_wallet_cli show s1                    # amount, fee, total, and the two commands
logosctl call monero_wallet_cli confirm s1                 # broadcast
logosctl call monero_wallet_cli cancel s1                  # or withdraw; a preview expires unsent after 120 s
```

Watch previews as they appear — including one built by another module:

```bash
logosctl watch monero_wallet_cli --event prompt
```

## Coming from `monero-wallet-cli`

| `monero-wallet-cli` | here |
|---|---|
| `balance` | `balances <account>` |
| `address` | `receive_info <account>` |
| `address new [<label>]` | `address_new <account> str:<label>` |
| `transfer <address> <amount>` | `transfer <address> str:<amount>` → review → `confirm <id>` |
| `show_transfers` | `history` |
| `seed` | `reveal_seed @<file>` |
| `viewkey` | `reveal_view_key @<file>` |
| `status` | `wallet_status` (`status` here reports this module's roles and queue) |
| `password` | `change_password @<old> @<new>` |
| `exit` | `close_wallet` |

Not built, deliberately: mining, multisig, proofs (`get_tx_key` / `check_tx_key`), `sweep_all`,
integrated addresses and the address book. Nothing here is offline signing — the engine signs when
it **builds** the preview, and `confirm` governs **broadcast**.

## Arguments

- **Passwords:** `@file` (out of shell history and `ps`) or `str:…`. Never bare — `1234` is coerced
  to a number. Exactly one trailing newline is stripped from a file, and the copy is wiped after use.
- **JSON documents** (`prepare_send`, `restore_from_*`): one quoted string or `@file` — never
  `json:`; the backend takes text and parses it itself.
- **Amounts, names and labels** that look numeric: `str:`.

**Do not `logosctl watch monero_wallet_cli` bare** on a shared terminal: the daemon publishes every
method reply as an event on the module's channel, and a `reveal_seed` reply is the seed. Pass
`--event prompt`.
