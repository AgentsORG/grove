# Changelog

## [0.1.0] - 2026-11-06

The first release of Grove: source control for people and agents, with one graph underneath.

### The record

- Zero files are split into declarations with stable, deterministic ids; every declaration is addressed by the SHA-256 of its canonical source. The same code builds the same graph on every machine.
- Other text files are kept byte for byte, and binary files are stored too, so nothing is ever rewritten or dropped.
- Edges come from references: exact in Zero code, inferred (from names in backticks) in docs.

### Changes and landing

- A change records its ops, its intent, its actor (an agent always acts for a named person) and its footprint: what it writes, rewrites and reads.
- Landing replays a change on whatever reached main since it began. Separate work lands together, dependent work is ordered after, renames reach call sites written later, and two edits to different lines of one function merge.
- Only two different edits to the same lines are a conflict, and it's saved for anyone to resolve.
- Checks (`grove init --zero` adds `zero check`) run on the combined result before main moves.

### Every git tool, in Grove's form

- `status`, `diff`, `new` (record, with versions), `land`, `sync`, `log`, `show`, `why`, `find`, `graph`, `cat`.
- `undo` and `ops` (an operation log: every action can be undone), `restore`, `shelve` / `unshelve`.
- `revert`, `pick`, `tag`, `lane new|use|land|drop` (branches), `evolve`, `shortlog`, `bisect`.
- `conflicts`, `resolve`, `verify`.
- Remotes: `clone`, `remote`, `fetch`, `pull`, `push` (a change lands on the remote with the same engine, or waits for review).

### Git all the way down

- `grove git export` writes main as git commits, one per landing, with Grove trailers, plus tags.
- `grove git push` sends them to GitHub, GitLab or any git host.
- `grove git import` brings a git repository's history in, keeping the original commit ids.
- `grove git pull` brings in commits made on the git side, and merges them when Grove's main also moved, so the next push fast-forwards.

### Install

- One-line installers for macOS, Linux and Windows, prebuilt binaries for x86_64 and arm64, and an agent skill: `npx skills add AgentsORG/grove`.

[0.1.0]: https://github.com/AgentsORG/grove/releases/tag/v0.1.0
