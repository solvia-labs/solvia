---
title: Rust API
---

Solvia's Rust crates are [published to crates.io][crates.io] and can be found
[on docs.rs with the "solana-" prefix][docs.rs].

[crates.io]: https://crates.io/search?q=solana-
[docs.rs]: https://docs.rs/releases/search?query=solana-

Some important crates:

- [`solana-program`] &mdash; Imported by programs running on Solvia, compiled
  to BPF. This crate contains many fundamental data types and is re-exported from
  [`solana-sdk`], which cannot be imported from a Solvia program.

- [`solana-sdk`] &mdash; The basic off-chain SDK, it re-exports
  [`solana-program`] and adds more APIs on top of that. Most Solvia programs
  that do not run on-chain will import this.

- [`solana-client`] &mdash; For interacting with a Solvia node via the
  [JSON RPC API](jsonrpc-api).

- [`solana-clap-utils`] &mdash; Routines for setting up a CLI, using [`clap`],
  as used by the main Solvia CLI.

[`solana-program`]: https://docs.rs/solana-program
[`solana-sdk`]: https://docs.rs/solana-sdk
[`solana-client`]: https://docs.rs/solana-client
[`solana-clap-utils`]: https://docs.rs/solana-clap-utils
[`clap`]: https://docs.rs/clap
