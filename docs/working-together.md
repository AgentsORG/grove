# Working together

<img src="images/workspaces.png" alt="Three terminals side by side: Claude Code, Codex and Meera each in their own workspace; Codex's change lands together with the other two" width="100%">

## Everyone gets a workspace

```bash
grove workspace add claude ../claude --as agent:claude-code --for harshit
grove workspace add codex  ../codex  --as agent:codex       --for rishitabh
grove workspace add meera  ../meera  --as meera
grove workspace list
```

A workspace is a folder holding main, plus who works in it. People and agents never share one. Inside a workspace, every command acts as its owner unless you say otherwise (`--as`, `--for`, or `GROVE_AS` and `GROVE_FOR`).

## People and agents

An agent always acts **for** a person. Every change records both, `grove log` and `grove why` show both, and the git export carries both (`Grove-Agent`, `Grove-For`). It's always clear who asked for what.

## The loop

```bash
grove sync                 # start from today's main; your unrecorded edits are kept
# ...edit...
grove status               # what changed, by declaration
grove diff                 # the text diff
grove new "Back off retries"
grove land
```

After a change lands, its workspace moves to the new main by itself.

## What landing tells you

| Result | Means |
|---|---|
| `Landed ... (direct)` | Nothing else landed since you started |
| `Landed ... (together with ...)` | Others landed meanwhile, in other parts of the graph |
| `Landed ... (ordered after ...)` | Others changed something yours depends on; yours was replayed on top and checked again |
| `merged inside <name>` | Someone else changed the same function, on different lines: merged |
| `Did not land: a check failed` | The combined result failed a check; main didn't move. Fix it, `grove new`, `grove land` |
| `Did not land: it clashes` | Someone set the same lines differently. See [Reviews and conflicts](review-and-conflicts.md) |

## Longer work: lanes

```bash
grove lane new checkout-v2
grove lane use checkout-v2      # this workspace now lands on the lane
# ...several changes, each landed on the lane...
grove lane use main
grove lane land checkout-v2     # the lane's work lands on main as one change
```

## Across machines

Push and pull through a GroveHub server or another folder ([Remotes](remotes.md)), or through any git host ([Git](git.md)).
