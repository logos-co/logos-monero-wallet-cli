//! monero_wallet_cli — the headless approver for `monero_wallet_backend`.

pub mod relay;
pub mod render;

#[cfg(feature = "logos_module")]
mod glue;
