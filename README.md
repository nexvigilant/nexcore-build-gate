# nexcore-build-gate

Build coordination and CI/CD gatekeeper for the NexVigilant Core kernel. This crate manages exclusive access to Cargo operations in multi-agent environments and uses content hashing to skip redundant builds.

## Intent
To prevent race conditions and resource contention during high-frequency development cycles. It ensures that only one agent or process can trigger a build or test run at a time, while optimizing performance through rigorous change detection (hashing).

## T1 Grounding (Lex Primitiva)
Dominant Primitives:
- **∂ (Boundary)**: The primary primitive for enforcing build gates and locking access.
- **κ (Comparison)**: Used for comparing content hashes to determine if a build is necessary.
- **π (Persistence)**: Manages the persistent lock file (`/tmp/nexcore-cargo.lock`) and hash cache.
- **→ (Causality)**: Orchestrates the sequential flow of hashing → locking → building → recording.

## Core Features
- **Exclusive Build Lock**: Prevents multiple `cargo` instances from running simultaneously.
- **Content Hashing**: Scans source files (`.rs`, `.toml`, `.lock`) to detect structural changes.
- **Result Caching**: Stores the outcome of the last successful build to avoid re-work.
- **Lock Timeout**: Gracefully handles stale locks or long-running builds with configurable timeouts.

## SOPs for Use
### Running a Coordinated Build
```rust
use nexcore_build_gate::{run_cargo, find_workspace_root};
use std::path::Path;

let root = find_workspace_root(Path::new(".")).unwrap();
let result = run_cargo(&root, &["build", "--release"], false)?;

if result.success {
    println!("Build verified and recorded.");
}
```

### Checking Lock Status
Before starting a long-running task, verify if the gate is clear:
```rust
use nexcore_build_gate::{lock_status, LockStatus};

if lock_status() == LockStatus::Held {
    println!("Build in progress, waiting...");
}
```

## License
Proprietary. Copyright (c) 2026 NexVigilant LLC. All Rights Reserved.
