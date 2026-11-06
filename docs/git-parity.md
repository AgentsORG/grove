# Git parity

Grove has to do everything git does, and work with everything that speaks git (GitHub, GitLab, Bitbucket, Cursor, CI, editors). This page tracks every command in git's own [`command-list.txt`](https://github.com/git/git/blob/master/command-list.txt), so nothing is missed by accident.

**Status:** ✅ works in this release · 🔜 planned · 🔁 covered by the git bridge (use git itself on the exported repository) · ➖ not needed, with the reason.

The rule while anything is 🔜: **you can still do it with git**, because `grove git export` writes a real git repository and `grove git pull` brings changes back.

## The bridge

| What | Status |
|---|---|
| Export main as git commits, one per landing, with authors, dates and Grove trailers | ✅ `grove git export` |
| Push main and tags to GitHub, GitLab or any git remote | ✅ `grove git push <url>` |
| Import a git repository's history, keeping author, date, subject and the original commit ids | ✅ `grove git import <src> <dest>` |
| Two-way sync: bring in commits made on the git side, merging when Grove moved too | ✅ `grove git pull <url>` |
| Undo in Grove moves git's `main` back | ✅ |
| Binary files | ✅ byte for byte |
| Tags | ✅ exported and pushed |
| Side branches of merge commits on import | 🔜 imported as lanes |
| Pull requests: open one from a Grove change | ✅ `grove git pr <url>` (opens it on GitHub with `gh`) |
| Git LFS and submodules | 🔜 kept as ordinary files today |

## Main porcelain

| git | Grove | Status |
|---|---|---|
| `init` | `grove init` | ✅ |
| `clone` | `grove clone URL` (GroveHub or a folder); `grove workspace add` locally | ✅ |
| `add`, `stage` | not needed: edits are read from the workspace when you record | ➖ |
| `status` | `grove status` | ✅ |
| `commit` | `grove new "<intent>"` (again for a new version) | ✅ |
| `restore`, `checkout -- <file>` | `grove restore [files]`, `grove cat <file>` | ✅ |
| `rm`, `mv` | delete or move the file, then `grove new` | ✅ |
| `diff` | `grove status` (by declaration), `grove diff [change]` (text) | ✅ |
| `log`, `show`, `shortlog`, `whatchanged`, `history` | `grove log`, `grove show`, `grove shortlog`, `grove ops` | ✅ |
| `blame`, `annotate`, `last-modified` | `grove why <name> [--all]`, through renames | ✅ per declaration |
| `grep` | `grove find <text>` | ✅ |
| `branch`, `switch`, `checkout` | `grove lane new\|use\|list\|drop` | ✅ |
| `worktree` | `grove workspace add\|list` | ✅ |
| `merge` | `grove land`, `grove lane land` (ordered by footprint, merged inside declarations) | ✅ |
| `rebase`, `replay` | `grove sync` (carries your edits onto main by meaning) | ✅ |
| `cherry-pick` | `grove pick <change>` | ✅ |
| `revert` | `grove revert <change>` | ✅ |
| `reset` | `grove undo`, `grove restore` | ✅ |
| `stash` | `grove shelve` / `grove unshelve` / `grove shelves` | ✅ |
| `tag`, `describe` | `grove tag` | ✅ |
| `fetch`, `pull`, `push` | `grove fetch`, `grove pull`, `grove push [--review]`; `grove git pull\|push` for git hosts | ✅ |
| `remote` | `grove remote add\|list` | ✅ |
| `bisect` | `grove bisect --run <cmd> [--good <at>]` | ✅ |
| `range-diff` | `grove evolve <change>` (every version of a change) | ✅ |
| `notes` | `grove note <change> "text"` | ✅ |
| `submodule` | pinned references to other repositories | 🔜 |
| `sparse-checkout` | `grove workspace add NAME --only PATH` (a partial workspace) | ✅ |
| `backfill` | lazy fetching from a server | 🔜 |
| `archive`, `bundle` | `grove git export`, then `git archive` / `git bundle` | 🔁 |
| `format-patch`, `am` | `grove git export`, then git's tools | 🔁 |
| `clean` | not needed: untracked files are left alone, ignored files never read | ➖ |
| `gc`, `maintenance`, `repack`, `prune` | content-addressed store with nothing to repack | ➖ |
| `gitk`, `gui`, `citool` | the GroveHub app | 🔜 |
| `scalar` | built in: no packfiles, objects fetched by address | ➖ |

## Ancillary commands

| git | Grove | Status |
|---|---|---|
| `config` | `.grove/config.json` (checks, remotes) | ✅ |
| `reflog` | `grove ops`, with `grove undo` | ✅ |
| `fsck`, `count-objects` | `grove verify` | ✅ |
| `verify-commit` | signed changes: `grove key new`, `grove key trust`, verified at every landing and in `grove show` | ✅ |
| `verify-tag` | signed tags: `grove tag` signs with your key, `grove tag` lists who signed each | ✅ |
| `rerere` | remembered resolutions: closing a conflict teaches Grove its answer | ✅ |
| `mergetool`, `difftool` | `grove conflicts <id>` shows base, main and yours | ✅ |
| `show-branch`, `cherry`, `merge-tree` | `grove lane list`, `grove drafts`, `grove log` | ✅ |
| `refs`, `pack-refs`, `replace` | `.grove/refs.json`, one atomic file | ➖ |
| `request-pull` | `grove git pr` | ✅ |
| `fast-export`, `fast-import`, `filter-branch` | `grove git export` / `grove git import` | ✅ |
| `help`, `version`, `bugreport`, `diagnose` | `grove --help`, `grove --version` | ✅ |
| `instaweb`, `gitweb` | GroveHub | 🔜 |

## Plumbing and helpers

git's plumbing (`cat-file`, `hash-object`, `ls-tree`, `update-ref`, `rev-parse`, `pack-objects`, `commit-tree`, …) exists to build porcelain on git's object format. Grove's store is different (declarations, snapshots, changes and landings, all SHA-256 addressed), so these don't map one to one. Each job has an owner in `grove-core`: the store, `refs.json`, `Repo::history`, and the git bridge, which drives git's own plumbing to write real git objects. `--json` output is the scripting interface.

## Foreign SCM interfaces

`svn`, `p4`, `cvsimport`, `archimport`, `quiltimport`, `send-email`, `imap-send`: 🔁 bring the history into git with git's tools, then `grove git import`.

## Protocols and servers

`daemon`, `http-backend`, `upload-pack`, `receive-pack`, `shell`: GroveHub serves Grove's own protocol (objects and proposals) over HTTP ✅; serving the git protocol from GroveHub is 🔜.
