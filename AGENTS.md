## Rust Quality Gates

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Docs Format

```bash
mdformat --wrap 80 --end-of-line keep .
```
