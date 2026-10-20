# Integration tests

These run against a real NEAR node: `near-workspaces` starts a sandbox, deploys
the actual contract binaries into it, and drives them through ordinary
transactions. Nothing is mocked except the NFT collection itself, which belongs
to a third party and is reproduced by `contracts/mock-nft`.

## Why these live in a package of their own

The tests talk to the contracts the way any other client does — JSON over RPC —
and load the compiled `.wasm` from disk, so they never need the contracts' Rust
types. Had they lived inside a contract crate, `cargo test` would compile that
contract for the host target, which near-sdk refuses:

```
Use `cargo near build` instead of `cargo build` to compile your contract
```

The usual escape is to hand near-sdk a `unit-testing` feature through
dev-dependencies. That works, but it means feeding test-only features into the
crate that produces the deployed binary — exactly what the parity test exists
to rule out. A separate package has no near-sdk dependency at all, so there is
nothing to leak.

## Build the binaries first

`cargo test` does **not** build them. Since near-sdk 5.27 a contract must be
compiled with `cargo near build`, which supplies the `--cfg near` flag; a plain
`cargo build --target wasm32-unknown-unknown` produces a binary that deploys
successfully and then traps at `new()`. near-workspaces' own `compile_project`
does a plain build, so it cannot be used here.

```bash
cd contracts/mock-nft && cargo near build non-reproducible-wasm --no-abi && cd -
cd contracts/staking  && cargo near build non-reproducible-wasm --no-abi && cd -
```

The artefacts land in each contract's own `target/near/`, and the tests read
them from there.

## Run

```bash
cd contracts/integration-tests
cargo test
```

**Only from here.** `cargo test` inside `contracts/staking` or
`contracts/marketplace` fails with near-sdk's `Use cargo near build instead of
cargo build` error, because it compiles the contract for the host target. That
is expected, not a broken project — those crates build only as wasm, with
`cargo near build`.

The first run downloads a sandbox node (a few hundred MB) and is slow. Later
runs reuse it.

### Windows cannot run these

The sandbox is a nearcore binary published only for linux-x86_64,
linux-aarch64 and darwin-arm64. On Windows `near-workspaces` fails at build
time with:

```
Could not install sandbox: UnsupportedPlatformError(
  "only linux-x86_64, linux-aarch64, and darwin-arm64 are supported")
```

No setting works around this — there is no Windows build to point
`NEAR_SANDBOX_BIN_PATH` at. Two options:

- **CI** (`.github/workflows/contracts.yml`) runs the suite on Linux at every
  push. This is the primary runner.
- **WSL2** for local iteration: `wsl --install -d Ubuntu`, then install Rust
  and cargo-near inside it and run from there.

One Windows note, in case anyone compiles the dependency tree there anyway:
`near-crypto` pulls `aws-lc-rs`, whose sibling `aws-lc-sys` assembles `.asm`
sources with NASM and needs version 3.x. An older NASM already on PATH —
Strawberry Perl ships 2.16 — is picked up first and fails. `where.exe nasm`
lists them in order.

Artefact paths can be overridden with `STAKING_WASM` and `MOCK_NFT_WASM` if your
build lands somewhere else.

## Two things worth knowing before writing a new test

**The collection's account name matters.** The staking contract hardcodes
`NFT_CONTRACT_ID = "nearlegion.nfts.tg"` and rejects `nft_on_transfer` from any
other caller, so the sandbox builds that exact name: a top-level `tg`, then
`nfts.tg`, then `nearlegion.nfts.tg`. Changing the constant for tests would mean
testing a binary other than the one deployed, which defeats the purpose.

**Waiting out a staking period is done by rewriting state, not by waiting.** A
7-day period is ~604,800 blocks; fast-forwarding that many is impractical. Tests
that need a matured stake patch `staked_at` in the record instead.

## Fixtures

`tests/fixtures/` holds bytecode downloaded from mainnet with
`near contract download-wasm`. It is committed deliberately — the chain stores
only the current code of an account, so a binary that has been replaced cannot
be fetched again. The parity test needs the exact binary that was running.
