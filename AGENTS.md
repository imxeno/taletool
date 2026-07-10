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
