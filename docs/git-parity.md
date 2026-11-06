# Git parity

Grove has to do everything git does, and work with everything that speaks git (GitHub, GitLab, Bitbucket, Cursor, CI, editors). This page tracks every command in git's own [`command-list.txt`](https://github.com/git/git/blob/master/command-list.txt), so nothing is missed by accident.

**Status:** ✅ works in this release · 🔜 planned, with the phase · 🔁 covered by the git bridge (you use git itself on the exported repository) · ➖ not needed, with the reason.

The rule while parity is incomplete: **anything Grove can't do yet, you can still do with git**, because `grove git export` writes a real git repository and `grove git import` reads one back.

## The bridge comes first

| What | Status |
|---|---|
| Export main to git, one commit per landing, author, date and Grove trailers | ✅ `grove git export` |
| Push to GitHub, GitLab or any git remote | ✅ `grove git push <url>` |
| Import a git repository's first-parent history, keeping author, date and subject | ✅ `grove git import <src> <dest>` |
| Undo in Grove moves git's `main` back | ✅ |
| Binary files (images, fonts, archives) | 🔜 next: chunked storage, carried through export and import |
| Exported commits reuse the original SHAs after an import | 🔜 store the source commit and reuse it when the tree is unchanged |
| Merge commits and side branches on import (beyond first-parent) | 🔜 imported as lanes |
| Two-way sync: pull new git commits made on GitHub into Grove | 🔜 phase 2, `grove git pull` |
| Pull requests: open a PR from a Grove change | 🔜 phase 2, through the host's API |
| Git LFS pointers | 🔜 with binary files |
| Submodules | 🔜 kept as pinned references |

## Main porcelain

| git | Grove | Status |
|---|---|---|
| `init` | `grove init` | ✅ |
| `clone` | `grove workspace add` (local); `grove clone` from a server | ✅ local · 🔜 phase 2 remote |
| `add`, `stage` | not needed: edits are read from the working copy when you record | ➖ by design |
| `status` | `grove status` | ✅ |
| `commit` | `grove new "<intent>"` | ✅ |
| `restore`, `checkout <file>` | `grove cat <file>`; `grove restore` | ✅ cat · 🔜 restore |
| `rm`, `mv` | delete or move the file, then `grove new` | ✅ (moves of a whole file 🔜 tracked as one op) |
| `diff` | `grove status` (by declaration); text diff | ✅ by declaration · 🔜 `grove diff --text` |
| `log`, `show`, `shortlog`, `whatchanged` | `grove log`, `grove show` | ✅ log, show · 🔜 shortlog |
| `blame`, `annotate`, `last-modified` | `grove why <name> [--all]` | ✅ per declaration · 🔜 per line |
| `grep` | `grove find` over the graph | 🔜 phase 1.x |
| `branch`, `switch`, `checkout` | workspaces (one per person or agent); named lanes | ✅ workspaces · 🔜 lanes |
| `worktree` | `grove workspace add/list` | ✅ |
| `merge` | `grove land` (ordered by footprint, replayed by meaning) | ✅ |
| `rebase`, `replay` | `grove sync` (carries your edits onto main) | ✅ |
| `cherry-pick` | `grove pick <change>` (replays its ops) | 🔜 phase 1.x |
| `revert` | `grove revert <change>` (applies the inverse ops) | 🔜 phase 1.x |
| `reset` | `grove undo`; `grove restore` | ✅ undo · 🔜 restore |
| `stash` | recorded drafts; `grove shelve` | ✅ drafts · 🔜 shelve |
| `tag`, `describe` | signed tags pinned to a landing | 🔜 phase 2 |
| `fetch`, `pull`, `push` | `grove git push` today; `grove sync` with a server in phase 2 | ✅ to git hosts · 🔜 Grove server |
| `remote` | `grove git push <url>`; named remotes | ✅ by URL · 🔜 named |
| `bisect` | `grove bisect`, reusing recorded evidence | 🔜 phase 1.x |
| `range-diff` | `grove evolve <change>` (versions of one change) | 🔜 (versions are already stored) |
| `notes` | notes on changes | 🔜 phase 2 |
| `submodule` | pinned references to other repositories | 🔜 |
| `sparse-checkout` | partial workspaces (only the graph you need) | 🔜 phase 2 |
| `archive`, `bundle` | `grove git export`, then `git archive` or `git bundle` | 🔁 · 🔜 native bundles |
| `format-patch`, `am` | change bundles by file or email | 🔁 · 🔜 native |
| `clean` | not needed: untracked files are left alone, ignored files never read | ➖ |
| `gc`, `maintenance`, `repack`, `prune` | content-addressed store, compaction later | ✅ nothing to do yet · 🔜 compaction |
| `backfill` | lazy fetching from a server | 🔜 phase 2 |
| `history` | `grove log`, `grove ops` (every operation, not only commits) | ✅ |
| `gitk`, `gui`, `citool` | the Grove app | 🔜 phase 2 |
| `scalar` | built in: no full checkouts, no packfiles to maintain | 🔜 phase 2 server |

## Ancillary commands

| git | Grove | Status |
|---|---|---|
| `config` | `.grove/config.json` (checks); `grove config` | ✅ file · 🔜 command |
| `reflog` | `grove ops` (the operation log, with undo) | ✅ |
| `fsck`, `verify-commit`, `verify-tag`, `count-objects` | `grove verify` | ✅ hashes · 🔜 signatures |
| `rerere` | remembered conflict resolutions | 🔜 phase 2 |
| `mergetool`, `difftool` | the conflict view in the app; `grove conflicts <id>` | ✅ text · 🔜 app |
| `show-branch`, `cherry`, `merge-tree` | `grove log`, `grove drafts` | ✅ partly |
| `refs`, `pack-refs`, `replace` | `.grove/refs.json` (one atomic file) | ➖ by design |
| `remote`, `request-pull` | through the host's API | 🔜 phase 2 |
| `fast-export`, `fast-import`, `filter-branch` | `grove git export` / `import` | ✅ |
| `blame`, `annotate` | `grove why` | ✅ |
| `help`, `bugreport`, `diagnose`, `version` | `grove --help`, `grove --version` | ✅ help, version |
| `instaweb`, `gitweb` | the Grove app | 🔜 |

## Plumbing and helpers

git's plumbing (`cat-file`, `hash-object`, `ls-tree`, `update-ref`, `rev-parse`, `pack-objects`, `index-pack`, `commit-tree`, …) exists to build porcelain on top of git's object format. Grove's store is different (declarations, snapshots, changes, landings, all SHA-256 addressed), so these don't map one to one. Every job they do has an owner: reading and writing objects (`grove-core::store`), refs (`refs.json`, atomic), walking history (`Repo::history`), and the git bridge, which calls git's own plumbing to produce real git objects. `grove --json` output is the scripting interface.

## Foreign SCM interfaces

`svn`, `p4`, `cvsimport`, `archimport`, `quiltimport`, `send-email`, `imap-send`: 🔁 import to git with git's tools first, then `grove git import`.

## Protocols and servers

`daemon`, `http-backend`, `upload-pack`, `receive-pack`, `shell`: 🔜 phase 2. The Grove server speaks its own API for graph queries and landing, and serves git over the bridge so existing clients keep working.
