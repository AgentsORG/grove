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
| Text files | Byte for byte |
| Binary files | Byte for byte |
| Authors, dates, messages | Yes |
| Tags | Yes |
| Merge commits | Pull merges are written as real merge commits; import follows first parents |
| Submodules, LFS pointers | Kept as files today; see [git parity](git-parity.md) |
