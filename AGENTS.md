## Client Naming

Refer to the game client as "NosTale" or "the client" in code, tests,
documentation, and CLI output. Describe client behavior directly.

## Rust Quality Gates

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Docs Format

```powershell
dprint fmt
dprint check
```
