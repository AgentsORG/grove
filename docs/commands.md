# Command reference

Every command takes `--as WHO` (`agent:codex`, `person:meera`, or a name), `--for PERSON`, and `--json`. `grove <command> --help` shows its options.

## Start

| Command | Does |
|---|---|
| `grove init [--zero] [--check NAME=CMD]` | Start a repository from the current folder. `--zero` adds `zero check`; `--check` adds any command |
| `grove workspace add NAME [PATH]` | A new folder holding main, for a person or an agent |
| `grove workspace list` | Workspaces, their owners and drafts |
| `grove clone URL [DIR]` | Copy a remote repository |

## Everyday

| Command | Does |
|---|---|
| `grove status` | What this workspace changed, by declaration |
| `grove diff [CHANGE]` | A git-style text diff of your edits, or of a change |
| `grove new "<intent>"` | Record your edits as a change; again for a new version |
| `grove land [CHANGE]` | Replay, order, check and land |
| `grove sync` | Bring the workspace up to main, keeping your edits |
| `grove drafts` | Recorded changes not yet landed |

## History

| Command | Does |
|---|---|
| `grove log [-n N]` | Main's landings |
| `grove show CHANGE` | A change's actor, footprint and ops |
| `grove why NAME [--all]` | Who changed a declaration, for whom and why |
| `grove find TEXT` | Lines on main containing TEXT |
| `grove evolve CHANGE` | Every version of a change |
| `grove shortlog` | Landed changes per person and agent |
| `grove cat FILE` | A file as it is on main |
| `grove graph [--dot \| --json]` | The program graph |
| `grove bisect --run CMD [--good AT]` | The first landing where CMD fails |

## Undo and move work

| Command | Does |
|---|---|
| `grove undo` | Undo the last operation |
| `grove ops [-n N]` | The operation log |
| `grove restore [FILES]` | Discard unrecorded edits |
| `grove shelve [NAME]`, `grove unshelve [NAME]`, `grove shelves` | Put edits aside and bring them back |
| `grove revert CHANGE` | Undo a landed change as a new change |
| `grove pick CHANGE` | Apply a change from elsewhere |
| `grove tag [NAME] [--at AT] [--delete]` | Name a landing; list tags |
| `grove lane new NAME [--from AT]` | Start a lane |
| `grove lane use NAME` | Point this workspace at a lane (`main` to go back) |
| `grove lane land NAME` | Land a lane's work on main as one change |
| `grove lane list`, `grove lane drop NAME` | List and delete lanes |

## Trust and review

| Command | Does |
|---|---|
| `grove key new` | Make a signing key for whoever is acting (kept in `~/.grove/keys`) |
| `grove key show` | Print your public key |
| `grove key trust` | Trust your key in this repository |
| `grove approve CHANGE` | Approve a draft's current version (people only, not your own change) |
| `grove config [KEY VALUE]` | Review rules: `review.signature`, `review.capabilities`, `review.hot_links`, `review.protected`, `review.require_signed` |
| `grove note CHANGE "text"` | A note on a change |
| `grove who` | Who is working on what, in every workspace |

## Conflicts and integrity

| Command | Does |
|---|---|
| `grove conflicts [ID]` | Open conflicts; one in full |
| `grove resolve ID` | Close a resolved conflict |
| `grove verify` | Check every stored object against its address |

## Remotes

| Command | Does |
|---|---|
| `grove remote add NAME URL`, `grove remote list` | Named remotes: GroveHub or a folder |
| `grove fetch [REMOTE]` | Get what's new |
| `grove pull [REMOTE]` | Fetch, move main, sync |
| `grove push [REMOTE] [--review] [--change C]` | Send landings and this workspace's change |

## Git

| Command | Does |
|---|---|
| `grove git export` | Write main as git commits in `.grove/git` |
| `grove git push URL [--force]` | Export, then push main and tags |
| `grove git pull URL` | Bring in commits made on the git side |
| `grove git import SRC DEST [--last N]` | A new Grove repository from a git repository's history |
| `grove git pr URL [--change C]` | Push a change as branch `grove/<change>` and open a pull request (GitHub, with `gh`) |

`AT` is a tag, a lane, a landing hash prefix or a change id.
