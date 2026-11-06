# Git

<img src="images/gitbridge.png" alt="grove git push, then git log showing each landing as a commit by its agent or person, with Grove trailers on one commit" width="100%">

GitHub, GitLab, Bitbucket, CI systems, editors and code review tools all speak git. Grove speaks it too, both ways.

## Push

```bash
grove git push git@github.com:you/acme-api.git
```

Every landing becomes one git commit on `main`:

- the **author** is whoever made the change (`codex@agents.grove` for an agent, `meera@people.grove` for a person),
- the **message** is the intent,
- the **trailers** carry the rest: `Grove-Change`, `Grove-Mode` (direct, together, ordered), `Grove-Agent`, `Grove-For`, `Grove-Evidence`.

Tags are pushed as git tags, and lanes as branches of the same name. Undo in Grove moves git's `main` back on the next push (use `--force` when the remote already has the undone commit).

`grove git export` writes the same history to `.grove/git` without pushing, for any git tool to read.

## Pull requests

```bash
grove git pr git@github.com:you/acme-api.git
```

Pushes this workspace's change as branch `grove/<change>` (one commit on top of its base, with Grove trailers) and, when the remote is on GitHub and the GitHub CLI (`gh`) is installed, opens a pull request for it. Elsewhere it pushes the branch and you open the pull request on your host.

## Pull

```bash
grove git pull git@github.com:you/acme-api.git
```

Brings in commits made on the git side, for example a pull request merged on GitHub:

- If Grove's main hasn't moved, they land directly and keep their commit ids.
- If it has, each commit is replayed on top like any change (ordered, checked) and exported as a **merge commit**, so the next `grove git push` fast-forwards instead of being refused.

## Import

```bash
grove git import ~/code/old-repo ~/code/acme-api
grove git import ~/code/old-repo ~/code/acme-api --last 200   # only recent history
```

Turns a git repository's first-parent history into Grove changes, keeping each commit's author, date and subject. Every other branch with commits of its own becomes a lane that starts where it left the checked-out branch (`main` itself, when it isn't checked out, becomes `git-main`); branches already merged are skipped. When Grove reproduces a commit's tree exactly, the commit keeps its id, so importing and exporting again gives back the same history and the same branches.

## What carries over

| | |
|---|---|
| Text files | Byte for byte, with no line-ending conversion and no `.gitattributes` filters on the way out |
| Zero files | Split into declarations when they're formatted (LF, one blank line between declarations, a final newline); otherwise kept whole, byte for byte |
| Binary files | Byte for byte |
| Executables, symlinks | Yes, through `.grovemodes` |
| Submodules | Yes: the pinned commit is kept in `.grovemodes` and `.gitmodules` is an ordinary file. Grove doesn't fetch the submodule's own contents |
| LFS files | The pointer files git stores, unchanged; Grove doesn't fetch LFS contents |
| Authors, dates, messages | Yes |
| Branches | Unmerged branches become lanes; lanes go back out as branches |
| Tags | Yes |
| Merge commits | Pull merges are written as real merge commits; import follows first parents |

### `.grovemodes`

Plain files can't say that a file is executable, that it's a symlink, or that a folder is a submodule pinned at a commit. When a git repository has any of these, `grove git import` writes them to `.grovemodes` at the top of the repository:

```
# git modes Grove keeps for you: <mode> <submodule commit or -> <path>
100755 - scripts/release.sh
120000 - latest
160000 4f2c9e1a0b7d3c6e8f9a1b2c3d4e5f60718293a4 vendor/lib
```

It's versioned like any other file, so changing a mode is a change that lands. Export reads it back into git's tree modes, so the commits keep their ids. To bump a submodule, change its commit on that line.
