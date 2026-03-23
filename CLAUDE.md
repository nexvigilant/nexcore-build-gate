# AI Guidance — nexcore-build-gate

Build coordination and resource gatekeeper.

## Use When
- Automating build or test cycles in a shared environment.
- Preventing multiple agents from conflicting on `target/` directory access.
- Optimizing performance by skipping redundant compilation steps.
- Implementing "BridgeVerify" logic in a VDAG pipeline.

## Grounding Patterns
- **Lock Management (∂)**: Never call `cargo` directly in an automated script; always use `run_cargo()` or wrap the call in a `BuildLock` guard.
- **Hash Sensitivity (κ)**: Note that `hash_source_dir` includes file paths and counts; moving a file will trigger a re-build even if content is identical.
- **T1 Primitives**:
  - `∂ + π`: Root primitives for persistent gating.
  - `κ + →`: Root primitives for change detection and execution flow.

## Maintenance SOPs
- **Lock Files**: If a build crashes, the lock file in `/tmp/` may persist. Use `lock_status()` to diagnose and manually clear if necessary.
- **Extensions**: Only `.rs`, `.toml`, and `.lock` files are hashed. If adding a new file type (e.g., `.σ`), you MUST update `HASH_EXTENSIONS` in `src/lib.rs`.
- **Result Files**: Build results are cached in `/tmp/nexcore-cargo.result`. This file is volatile across reboots.

## Key Entry Points
- `src/lib.rs`: `run_cargo()`, `BuildLock`, and hashing logic.
- `src/main.rs`: The `build-gate` CLI entry point.
