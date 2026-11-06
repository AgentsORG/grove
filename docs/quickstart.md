# Quick start

Five minutes: a repository, two workspaces, two pieces of work landed at the same time.

## 1. Start a repository

In a folder with code (or an empty one):

```bash
grove init --as harshit --zero   # --zero adds `zero check` before every landing
```

Grove reads every file. Zero files (`.0`) are split into declarations; everything else is tracked file by file, byte for byte.

```bash
grove graph          # what Grove sees: declarations and who calls whom
```

## 2. One workspace per person or agent

```bash
grove workspace add codex ../codex --as agent:codex --for rishitabh
grove workspace add meera ../meera --as meera
```

Each workspace is a folder holding main. An agent always works *for* someone; Grove records both.

## 3. Work at the same time

Edit files in each folder as usual, then record the change with one line of intent:

```bash
cd ../codex && grove new "Back off retries"
cd ../meera && grove new "Checkout button copy"
```

`grove status` and `grove diff` show what you changed before you record.

## 4. Land

```bash
cd ../codex && grove land
cd ../meera && grove land
```

```text
Landed c_c7e86c "Back off retries" (direct)
  ok   zero check
Landed c_0d9501 "Checkout button copy" (together with c_c7e86c)
  ok   zero check
```

The two changes touched different parts of the program, so they landed together. If Meera's change had read something Codex's changed, it would have landed *ordered after* it, replayed and checked again.

## 5. Look back

```bash
grove log
grove why retryDelay      # who changed it, for whom, and why
grove undo                # changed your mind? any operation can be undone
```

## 6. Share it

```bash
grove git push git@github.com:you/acme-api.git
```

Every landing becomes a git commit, so GitHub, GitLab and every git tool keep working. Next: [Working together](working-together.md).
