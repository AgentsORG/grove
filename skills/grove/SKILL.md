---
name: grove
description: Use Grove for source control when working beside other agents and people. Record changes with intent, land them safely, and read history by meaning. Use in any repo with a .grove folder.
---

# Grove

Grove is source control for people and agents, with one graph underneath. Code is split into declarations (functions, constants, types, tests), each with a stable id. A change records its edits, its intent, who made it and who it was for, and what it touches. Landing replays a change on whatever reached main since it began: separate work lands together, dependent work is ordered after, and only two different edits to the same lines are a conflict. Checks run on the combined result before main moves. Every operation can be undone.

## Install (only if `grove` is missing)

Use the `grove` already on PATH or at `~/.grove/bin/grove` when there is one.

```sh
command -v grove >/dev/null 2>&1 || curl -fsSL https://raw.githubusercontent.com/AgentsORG/grove/main/install.sh | sh
export PATH="$PATH:$HOME/.grove/bin"
grove --version
```

On Windows PowerShell: `irm https://raw.githubusercontent.com/AgentsORG/grove/main/install.ps1 | iex`

## Who you are

Always say who is acting. You are an agent, working for a person:

```sh
export GROVE_AS=agent:<your-harness>   # agent:claude-code, agent:codex, agent:cursor ...
export GROVE_FOR=<person>              # the human who asked for this work
```

Or pass `--as agent:<name> --for <person>` on each command. Changes record both, and `grove why` shows them.

## Work in your own workspace

Never share a folder with another agent. Ask for, or make, your own workspace:

```sh
grove workspace add <name> ../<name> --as agent:<harness> --for <person>
cd ../<name>
```

## The loop

1. `grove sync`: start from the latest main (your unrecorded edits are kept).
2. Edit files normally.
3. `grove status`: what you changed, by declaration. `grove diff` shows the text diff.
4. `grove new "<one line of intent>"`: record the change. Recording again makes a new version of the same change.
5. `grove land`: replay, order, check and land it. Read the result:
   - `Landed ... (direct | together with ... | ordered after ...)`: done. The workspace moves to the new main.
   - `Did not land ...: a check failed`: fix what the check output says, `grove new` again, `grove land` again.
   - `Did not land ...: it clashes`: run `grove conflicts <id>` to see base, main and yours. Run `grove sync`, combine both intents by hand, then `grove new`, `grove land` and `grove resolve <id>`.

Use `--json` on any command for machine-readable output.

## Reading history

- `grove log`, `grove show <change>`: what landed, how, and why.
- `grove why <name> [--all]`: who last changed a function or constant, for whom, and why. It follows renames.
- `grove find <text>`, `grove graph --json`: search main, or read the program graph.
- `grove evolve <change>`: every version of a change.

## Keys and approvals

- `grove key new` once per agent identity: your changes are then signed. The person you work for trusts it with `grove key trust`.
- Agents never approve. When `grove land` says a change needs a person's approval, tell the person you work for and wait; don't work around it.
- `grove who` shows who is working on what, so you don't duplicate someone's work.

## Undo and the other tools

- `grove undo`: undo the last operation (a land, a record, a sync). `grove ops` lists them.
- `grove restore [files]`: discard unrecorded edits. `grove shelve` / `grove unshelve`: put edits aside.
- `grove revert <change>`: undo a landed change as a new change. `grove pick <change>`: apply one from elsewhere.
- `grove lane new|use|land <name>`: longer work beside main. `grove tag <name>`: name a release.
- `grove bisect --run "<test command>"`: find the landing that broke something.
- `grove note <change> "text"`: leave a note on a change.
- `grove git pr <url>`: open a pull request on GitHub from your change (only when asked).

## Rules

- Don't edit `.grove/`, and don't commit `zero.graph`; Grove manages both.
- Write intents a person can read: what changed, and why. Not "update file".
- Don't use `grove undo` on another person's or agent's operation without being asked.
- Don't push to a shared remote (`grove push`, `grove git push`) unless the person you work for asked.
- Zero projects: `grove init --zero` adds `zero check` to the checks that run before landing.

More: https://github.com/AgentsORG/grove (docs/, docs/commands.md).
