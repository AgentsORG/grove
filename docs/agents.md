# For agents

Grove is built for agents working beside other agents and people.

## Install the skill

```bash
npx skills add AgentsORG/grove
```

The skill ([skills/grove/SKILL.md](../skills/grove/SKILL.md)) tells an agent how to install `grove` when it's missing, how to work in its own workspace, how to land, and what to do when a land doesn't go through.

## Say who you are

```bash
export GROVE_AS=agent:claude-code   # your harness
export GROVE_FOR=harshit            # the person you're working for
```

Or `--as` and `--for` on any command. Workspaces remember their owner, so inside your workspace you don't need either.

## Machine-readable output

Add `--json` to `status`, `new`, `land`, `show` and `graph`:

```bash
grove land --json
```

```json
{"landed": "c_c7e86c", "mode": {"mode": "together", "with": ["c_a5b7b9"]}, "evidence": [{"check": "zero check", "ok": true}]}
```

Exit codes: `0` landed, `2` a check failed, `3` a conflict was saved, `1` anything else.

## The rules

- Work in your own workspace. Never edit `.grove/`.
- Write intents a person can read: what changed, and why.
- When a land is refused, fix and land again; when it conflicts, read `grove conflicts <id>` and combine both intents.
- Don't undo, push or land on another person's behalf unless asked.
