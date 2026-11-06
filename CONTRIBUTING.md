# Contributing to Grove

Thanks for helping. Grove is early, so the most useful thing you can do is pick a 🔜 from [docs/git-parity.md](docs/git-parity.md) and make it ✅.

## Build and test

```bash
cargo build
cargo test                 # unit tests, end-to-end scenarios, the git bridge (needs git)
bash examples/demo.sh      # the whole story on real folders (zero check runs if zero is on PATH)
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

## Layout

| | |
|---|---|
| `crates/grove-core` | The engine: store, graph, ops, diff, landing, op log, git bridge |
| `crates/grove-cli` | The `grove` command |
| `crates/grove-core/tests/scenarios.rs` | The behaviour Grove promises, as stories. Add one for every new promise. |
| `docs/design.md` | How it works. Update it when behaviour changes. |
| `docs/git-parity.md` | What works compared with git. Update it in the same PR. |

## Rules of the road

- **Every promise has a scenario test.** If a change lands, conflicts, orders or renames differently, a test in `scenarios.rs` says so.
- **Addresses are content.** Nothing machine-local (clock, path, process id) may go into a declaration id or a snapshot. Two machines with the same code must build the same graph; a test checks this.
- **Never lose work.** A failing check, a conflict or a crash must leave main where it was and keep the change.
- **Plain words** in messages and docs: say what happened and what to do next.
- Agents are welcome contributors. Say in the PR which agent did the work and for whom, the same way Grove records it.

## License

By contributing you agree your work is licensed under [Apache-2.0](LICENSE).
