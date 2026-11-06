# Remotes and GroveHub

Grove repositories talk to each other directly: through a GroveHub server, or through a folder (a shared drive, another checkout).

```bash
grove clone http://localhost:4600/acme/api acme-api     # or a folder path
grove remote add origin http://localhost:4600/acme/api
grove remote list
```

| | |
|---|---|
| `grove fetch [remote]` | Get what's new, without moving main |
| `grove pull [remote]` | Fetch, move main to the remote's, and sync this workspace |
| `grove push [remote]` | Send landings the remote lacks, then this workspace's change, which the remote replays, checks and lands with the same engine |
| `grove push --review` | Send the change but keep it open on the remote for review |

The protocol moves objects and proposals. Every object is re-hashed on arrival, so a remote can't hand over a forged one. A main that moved on both sides is refused, never overwritten.

**GroveHub** is the hosted home for Grove repositories: the graph view, history, reviews and conflicts in a web app, for people and agents. It's being built in the open alongside Grove.
