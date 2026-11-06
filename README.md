<p align="center">
  <img src="docs/images/hero.png" alt="Grove: five people and agents working on one program graph. Four changes land together; Cursor's change reads what Codex changed, so it lands after it. A terminal shows the same landings from the command line." width="100%">
</p>

<h1 align="center">Grove</h1>

<p align="center"><b>Source control for people and agents, with one graph underneath.</b></p>

<p align="center">
  <a href="LICENSE">Apache-2.0</a> ·
  <a href="docs/design.md">How it works</a> ·
  <a href="docs/git-parity.md">Git parity</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  Built in the open by <a href="https://agents.org.in">Agents</a>
</p>

---

Agents now write code in parallel, many at a time. Git was built for people trading patches: it stores text, compares lines, and calls two edits a conflict when they sit near each other, even when they change different things. Grove keeps everything git gives you and changes what a change *is*.

- **The program graph is the record.** Code is split into declarations: functions, constants, types, tests. Each one has a stable id and is addressed by the SHA-256 of its source. Edges come from references. Files and git commits are views of the graph.
- **A change knows what it touches.** Every change records its edits, its intent, who made it and who it was for. It also records its **footprint**: what it writes, what it reads, and which call sites its renames rewrite.
- **Separate work lands together.** Work that depends on something that just changed is replayed on top of that change and checked again. Only two different edits to the same declaration count as a conflict. That conflict is saved, and anyone, person or agent, can pick it up.
- **Renames carry the meaning.** A rename reaches call sites someone added after the rename was written.
- **Nothing lands unchecked.** Checks such as `zero check` run on the combined result before main moves.
- **Every operation can be undone.** Each one is logged with the state before and after it.
- **It still works with git.** `grove git push` sends a real git history to GitHub, GitLab or any other git host. `grove git import` brings a git repository's history in.

Grove is built around [Zero](https://zerolang.ai), the programming language for agents, whose programs are graphs already. Other files are tracked too: as whole files today, with graph edges inferred from the names they mention in backticks.

## Parallel work, on the graph

<img src="docs/images/graph.png" alt="Grove's graph view: rings mark each person's or agent's change; separate rings land together, a shared edge orders Cursor after Codex" width="100%">

Each ring is someone's change, marked with their face or their agent's mark. People are round; agents are square and wear the face of the person they work for. Rings that don't touch land together. A shared edge puts one change after the other. Shaded areas are clusters of code that work together.

## History is a graph too

<img src="docs/images/history.png" alt="Grove's history view: changes that landed in the same moment share a bracket, a release tag, a lane joining main, and a card showing a change's author, sponsor, approver, signature, ops and evidence" width="100%">

## Review reads the meaning first

<img src="docs/images/review.png" alt="A review in Grove: a rename described in three plain sentences, the source diff underneath, evidence, and an approval required because a public signature changed" width="100%">

## When work really collides

<img src="docs/images/conflict.png" alt="A conflict in Grove: two people changed the same line of retryDelay; one statement merged on its own, and Claude Code proposes a combined line that passes the checks" width="100%">

> The four screens above are design previews of the Grove app, which is being built as **GroveHub**. Everything in the command-line section below works today.

## Try it

```bash
git clone https://github.com/AgentsORG/grove && cd grove
cargo install --path crates/grove-cli
bash examples/demo.sh   # the whole story: four agents and two people, real landings, real zero check
```

```text
$ grove init --as harshit --zero
Started a Grove repository in ~/acme-api
  8 declarations, 6 edges, as harshit

$ grove workspace add codex  ../codex  --as agent:codex  --for rishitabh
$ grove workspace add cursor ../cursor --as agent:cursor --for dev

# both agents edit at once, each in its own folder
$ cd ../codex  && grove new "Back off retries"
Recorded c_c7e86c v1 "Back off retries" as codex for rishitabh
  writes   retryDelay
  reads    fetchAll

$ cd ../cursor && grove new "Use retryDelay in fetchAll"
$ cd ../codex  && grove land
Landed c_c7e86c "Back off retries" (direct)
  ok   zero check
$ cd ../cursor && grove land
Landed c_c547e6 "Use retryDelay in fetchAll" (ordered after c_c7e86c)
  ok   zero check

$ grove why retryDelay
c_c7e86c  codex for rishitabh  "Back off retries"  (direct)

$ grove git push git@github.com:you/acme-api.git
pushed main to git@github.com:you/acme-api.git
```

## Commands

| Command | What it does |
|---|---|
| `grove init [--zero] [--check NAME=CMD]` | Start a repository from the current folder |
| `grove workspace add NAME [PATH] --as WHO [--for PERSON]` | A folder of its own for a person or an agent |
| `grove status` | What this workspace changed, by declaration |
| `grove new "<intent>"` | Record the edits as a change; record again for a new version |
| `grove land [CHANGE]` | Replay, order, check, land |
| `grove sync` | Bring this workspace up to main, keeping your own edits |
| `grove log`, `grove show C`, `grove why NAME [--all]` | History and who changed what, and why |
| `grove cat FILE`, `grove graph [--dot \| --json]` | Main's files and its program graph |
| `grove conflicts [ID]`, `grove resolve ID` | Saved conflicts |
| `grove undo`, `grove ops` | The operation log |
| `grove git export \| push URL \| import SRC DEST` | Work with git |
| `grove verify` | Check every object still matches its address |

`--as agent:codex --for rishitabh` (or the `GROVE_AS` and `GROVE_FOR` environment variables) says who is acting. `--json` gives machine-readable output for agents.

## Status

Early, and honest about it. This is phase 1: one machine, many workspaces, the git bridge. [docs/git-parity.md](docs/git-parity.md) checks every command in git's own command list and says what works today, what's planned, and what isn't needed. The biggest gaps right now are binary files, a server for working across machines, and pull requests through the git hosts. [docs/design.md](docs/design.md) explains how it works and lists its current limits.

## Contributing

Every 🔜 on the [parity list](docs/git-parity.md) is a real task. Start with [CONTRIBUTING.md](CONTRIBUTING.md). Agents are welcome contributors: say which agent did the work and who it was for, the way Grove records it.

## License

[Apache-2.0](LICENSE) © 2026 Agents.
