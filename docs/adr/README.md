# Architecture Decision Records

This directory tracks architectural decisions that are expensive to reverse or likely to raise "why" questions for future contributors.

| ADR | Status | Decision |
| --- | --- | --- |
| [0001](0001-sqlite-writer-process.md) | Accepted | Use a dedicated Bun writer process as the only SQLite owner. |
| [0002](0002-initial-snapshot-json-history.md) | Accepted | Store initial history as indexed metric columns plus full snapshot JSON. |
