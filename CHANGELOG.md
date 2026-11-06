# Changelog

## [Unreleased]

### Added

- **Executables, symlinks and submodules** survive the git bridge, kept in a versioned `.grovemodes` file; LFS pointer files stay pointers.
- **Branches both ways.** `grove git import` brings every unmerged git branch in as a lane, starting where it left the checked-out branch, with its own commits and their original ids. `grove git export` and `push` write lanes back out as branches of the same name.
- **Exact edges from Zero's resolver.** With the `zero` compiler installed and a `zero.toml` in the repository, code edges come from `zero query --refs` instead of names: a parameter named like a function is no longer an edge, so fewer landings are ordered for nothing. Answers are cached per snapshot in `~/.grove/cache/edges`; without Zero, or when the code doesn't compile, Grove uses names. `grove graph` and the `grove: graph` evidence say which (`from zero` / `from names`). `GROVE_EDGES=names` turns it off, `GROVE_ZERO` points at a specific compiler.

### Fixed

- **Byte-exact git export.** Blobs are written with no filters, so neither `core.autocrlf` nor a repository's `.gitattributes` can change a file on the way out.
- **Zero files are no longer rewritten.** A `.0` file keeps its own line endings (LF or CRLF) and a missing final newline while still splitting into declarations; merges inside a declaration keep CRLF too. A file whose layout the declarations can't give back (extra blank lines between them) is kept whole, byte for byte. Importing and exporting Grove's own history gives back identical commit ids.

## [0.3.0] - 2026-11-06

### Added

- **Partial workspaces** (git's sparse checkout): `grove workspace add NAME --only PATH` holds only those paths. Everything else stays as main has it, so nothing outside looks deleted and sync never writes it.
- **Signed tags** (git's verify-tag): `grove tag` signs with your key; `grove tag` lists each tag and who signed it, and a signature moved to another tag doesn't verify.

## [0.2.0] - 2026-11-06

People stay in charge, and nothing is done twice.

### Added

- **Signed changes.** `grove key new` makes an ed25519 key for a person or an agent; changes are signed when they're recorded, and `grove key trust` lists a key in the repository. Every landing verifies the signature (a change altered after signing never lands), and `review.require_signed` refuses unsigned work.
- **Review rules and approvals.** Turn on what needs a person: a public signature changed, new file, network, process or HTTP access, a heavily linked declaration rewritten, or a protected path. `grove approve` records a person's yes to one version of a change; agents can't approve, nobody approves their own change, and a new version needs a new approval. Approvals are signed; with `review.require_signed`, only approvals signed by a trusted person's key count.
- **Remembered resolutions.** Closing a conflict teaches Grove its answer; the same clash resolves itself next time (git's rerere).
- **Notes** on changes (`grove note`), shown with `grove show`.
- **Presence**: `grove who` lists who is working on what in every workspace.
- **Pull requests**: `grove git pr` pushes a change as a `grove/<change>` branch and opens a pull request on GitHub with `gh`.
- `grove show` lists the signature, approvals and notes.

### Changed

- A refused landing says why in words: a failed check, a missing approval, or an untrusted signature.

## [0.1.0] - 2026-11-06

The first release of Grove: source control for people and agents, with one graph underneath.

### The record

- Zero files are split into declarations with stable, deterministic ids; every declaration is addressed by the SHA-256 of its canonical source. The same code builds the same graph on every machine.
- Other text files are kept byte for byte, and binary files are stored too, so nothing is ever rewritten or dropped.
- Edges come from references: names used in Zero code, and names in backticks in docs (inferred). Edges from Zero's own resolver are planned.

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

[0.3.0]: https://github.com/AgentsORG/grove/releases/tag/v0.3.0
[0.2.0]: https://github.com/AgentsORG/grove/releases/tag/v0.2.0
[0.1.0]: https://github.com/AgentsORG/grove/releases/tag/v0.1.0
