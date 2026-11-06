# How Grove works

## The record: two graphs

**The program graph.** Every tracked file is split into declarations. In Zero files (`.0`) that's each top-level function, constant, `use` line, test and type, with the comments directly above it. Any other text file is one declaration of kind `file`. Each declaration has:

- a **stable id** (`d_…`), which survives edits and renames,
- a name, kind, file and order,
- a **content address**: `sha256:` of its canonical source (trailing spaces trimmed, one final newline).

Edges are derived from content every time they're needed, never stored, so they can't disagree with it. A reference in Zero code is a `code` edge. A name in backticks in a non-code file is a `mention` edge, which is inferred and weaker. A snapshot of the program graph is the map of ids to entries.

**The change graph.** A **change** (`c_…`) is a list of ops on the program graph, plus the intent, the actor (and the person an agent acts for), the base it started from, and its **footprint**. A **landing** records one step of main: the snapshot it produced, its parent landing, the change, how it landed, and the evidence (check results). Main is a pointer to the latest landing.

Files, git commits and the app are generated from these two graphs.

## Ops

| Op | Means |
|---|---|
| `add` | A new declaration, with a fresh id |
| `replace` | New source for a declaration, expecting the content it had |
| `rename` | A new name for a declaration and every reference to it, wherever those references are when the op runs |
| `delete` | Remove a declaration, expecting the content it had |

People and agents edit files normally. `grove new` compares the working copy with the base, declaration by declaration, and writes ops: renames first (a declaration that reappears under a new name with every reference updated), then replaced bodies, deletions and additions. A call site that only differs by the rename gets no op of its own; the rename carries it.

## Footprints

- **writes**: declarations whose content the change sets,
- **rewrites**: references its renames update (these commute with other edits),
- **reads**: one edge away from anything it changes, in either direction (what it calls, what calls it).

## Landing

When a change lands, Grove collects every change that reached main since its base, then:

1. **Conflict.** If any of them wrote a declaration this change also writes, and the two results differ, the change doesn't land. A conflict object is saved (base, main and this change's version of each declaration) and main is untouched. If both set it to the same thing, that's not a conflict.
2. **Replay.** The ops apply to the current main. Renames that landed since are carried into this change's incoming source, so an edit written against the old name lands correctly. A `replace` checks that the declaration still holds what it expected, allowing for those renames.
3. **Order.** If anything that landed since touched what this change depends on (its footprint, or what its replay touched), the landing is **ordered after** those changes. Otherwise it landed **together** with them.
4. **Check.** The combined result is written to a scratch folder and every configured check runs there (`grove init --zero` adds `zero import . && zero check .`). If one fails, main doesn't move.
5. **Land.** The snapshot and landing are stored and main moves, as one atomic write of `refs.json` under the repository lock.

## The operation log

Every command that changes refs appends an entry to `.grove/oplog.jsonl` with the refs before and after. `grove undo` restores the refs from before the latest operation that hasn't been undone, and logs that too. Undoing a land puts main back and the change back in drafts.

## Storage

`.grove/objects/` holds immutable objects addressed by `sha256:` of a domain tag plus their bytes (`decl`, `snapshot`, `change`, `landing`, `conflict`). The algorithm is part of every address, so a later hash change can coexist with old objects. Writes go to a temporary file and are renamed into place. Reads verify the address. `grove verify` checks them all.

`zero.graph` is never tracked: Zero's node handles and graph hash are local to one machine (two copies of the same program end up with different ones), so Grove addresses code by its canonical source instead.

## The git bridge

`grove git export` writes `.grove/git`, a bare git repository with one commit per landing. Authors are the actors; trailers carry the change id, mode, agent, the person it was for, and evidence. Git's `main` always matches Grove's main, including after an undo. `grove git import` reads a git repository's first-parent history through git's own plumbing and lands one change per commit, keeping author, date and subject.

## Remotes

`grove_core::remote` is the protocol between repositories. It moves **objects** and **proposals**:

- **fetch** sends every object reachable from the remote's main that the receiver lacks. The receiver re-hashes each one against its domain and address, so a remote can't hand over a forged object.
- **push** fast-forwards the remote's main to history landed locally. A main that moved on both sides is refused, never overwritten.
- **propose** sends one change. The remote lands it with the same `Repo::land` (replay, order, check), or keeps it open for review. When it lands, the client fast-forwards and moves the workspace that held it.

Transports: a folder (`Folder`) in grove-core, HTTP in the CLI. A server hosts bare repositories (`Repo::init_bare`) whose first landing arrives by push.

## Merging inside a declaration

When two changes set the same declaration, Grove runs a line-level three-way merge of base, main and the incoming version. If they changed different lines, the merged text lands, recorded as `grove: merged inside <name>` evidence and ordered after the other change. Only overlapping edits that differ become a conflict.

## History tools

`revert` builds the inverse ops of a landed change (renames back first, so the rest see the original names) and lands them as a new change. `pick` replays a change's ops from wherever it is, based on the shared history. Lanes are named heads: a workspace on a lane lands there, and `lane land` lands the lane's whole difference from its fork point onto main as one change. Tags name landings. Shelves store unrecorded edits as a change object and replay them later. `bisect` materializes landings in scratch folders and halves the range.

## Phases

1. **Today (0.1)**: graph store, stable ids, SHA-256, footprints, land, sync, why, undo, merging inside declarations, binary files, revert, pick, lanes, tags, shelves, bisect, remotes, and the two-way git bridge.
2. **Next**: signed identities and delegation, presence, review rules, a merge queue across machines, pull requests through the git hosts, the GroveHub app.
3. **Then**: tree-sitter graphs for other languages (inferred edges), exact edges from Zero's own resolver, chunked storage for very large binaries, partial (lazy) workspaces.

## Known limits today

- Edges in Zero code come from names, not from Zero's resolver yet, so two declarations with the same name in different modules share edges.
- Files that aren't Zero code are one declaration each: two edits to different lines of a doc merge, but there are no finer graph edges inside them yet.
- Binary files are stored whole (base64 in the object store); very large binaries are slower than in git.
- Workspaces are full local copies; there is no partial (lazy) workspace yet.
- Checks run while the repository lock is held, so landings are serialized on one machine.
- `grove git import` follows first parents; side branches of merge commits are not imported as lanes yet.
