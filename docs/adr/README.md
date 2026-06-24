# Architecture Decision Records

This directory tracks architectural decisions that are expensive to reverse or likely to raise "why" questions for future contributors.

| ADR | Status | Decision |
| --- | --- | --- |
| [0001](0001-sqlite-writer-process.md) | Accepted | Use a dedicated Bun writer process as the only SQLite owner. |
| [0002](0002-initial-snapshot-json-history.md) | Accepted | Store initial history as indexed metric columns plus full snapshot JSON. |
| [0003](0003-bash-bootstrap-bun-install-wizard.md) | Accepted | Use a Bash bootstrap command center that launches a Bun setup wizard after Bun is available. |
| [0004](0004-rust-agent-sqlx-store.md) | Accepted | Add an additive Rust collector/agent path and use SQLx for Rust-side storage. |
| [0005](0005-rust-single-daemon-systemd-runtime.md) | Accepted | Use a single Rust daemon as the default systemd runtime while keeping the Bun split path as fallback. |
