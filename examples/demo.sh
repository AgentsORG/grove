#!/usr/bin/env bash
# The Grove deck's story, run for real: four agents and two people working on
# one Zero program at once. Needs `cargo` and, for the zero check, the zero
# compiler on PATH (https://zerolang.ai).
#
#   bash examples/demo.sh
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --quiet --manifest-path "$here/Cargo.toml"
grove="$here/target/debug/grove"
[ -x "$grove" ] || grove="$grove.exe"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
edit() { # edit FILE FROM TO: replace the first FROM with TO
  python3 - "$@" <<'PY' 2>/dev/null || python - "$@" <<'PY'
import sys; p, a, b = sys.argv[1:4]; t = open(p).read(); assert a in t, a; open(p, "w").write(t.replace(a, b, 1))
PY
import sys; p, a, b = sys.argv[1:4]; t = open(p).read(); assert a in t, a; open(p, "w").write(t.replace(a, b, 1))
PY
}

say "A Zero program, put under Grove by Harshit"
mkdir acme-api && cd acme-api
mkdir src docs
cat > zero.toml <<'EOF'
[package]
name = "acme-api"
version = "0.1.0"
license = "Apache-2.0"

[targets.cli]
kind = "exe"
main = "src/main.0"
defaultTarget = "linux-musl-x64"
devTarget = "host"
releaseProfile = "release-small"

[deps]

[profiles.dev]
inherits = "dev"

[profiles.release-small]
inherits = "release-small"
EOF
cat > src/main.0 <<'EOF'
pub fn main(world: World) -> Void raises {
    let n: u32 = fetchAll(3)
    let k: u32 = rateLimit(parse(n))
    check world.out.write("ok\n")
}

fn parse(b: u32) -> u32 {
    return b + 1
}

fn retryDelay(attempt: u32) -> u32 {
    return 100 * attempt
}

fn fetchAll(times: u32) -> u32 {
    return retryDelay(times)
}

fn rateLimit(key: u32) -> u32 {
    return key
}

fn checkoutLabel() -> u32 {
    return 7
}
EOF
printf '# API\n\nCall `rateLimit` before `fetchAll`.\n' > docs/api.md
zero_flag=""
command -v zero >/dev/null && zero_flag="--zero"
# shellcheck disable=SC2086 # empty when zero is not installed
"$grove" init --as harshit $zero_flag
"$grove" graph

say "Six workspaces: four agents, each working for someone, and two people"
"$grove" workspace add claude ../claude --as agent:claude-code --for harshit
"$grove" workspace add codex  ../codex  --as agent:codex  --for rishitabh
"$grove" workspace add cursor ../cursor --as agent:cursor --for dev
"$grove" workspace add gemini ../gemini --as agent:gemini-cli --for meera
"$grove" workspace add meera  ../meera  --as meera
"$grove" workspace add dev    ../dev    --as dev

say "Everyone edits at the same time"
edit ../claude/src/main.0 "return key" "return key % 1000"
edit ../meera/src/main.0  "return 7" "return 8"
edit ../gemini/docs/api.md "before \`fetchAll\`." "before \`fetchAll\`. Keys reset every hour."
edit ../codex/src/main.0  "return 100 * attempt" "return 100 * attempt * attempt"
edit ../cursor/src/main.0 "return retryDelay(times)" "return retryDelay(times) + 1"
(cd ../claude && "$grove" new "Per-key rate limit")
(cd ../meera  && "$grove" new "Checkout button copy")
(cd ../gemini && "$grove" new "Document the rate limits")
(cd ../codex  && "$grove" new "Back off retries")
(cd ../cursor && "$grove" new "Use retryDelay in fetchAll")

say "They land: separate work lands together; Cursor reads what Codex changed, so it is ordered after it"
(cd ../claude && "$grove" land)
(cd ../meera  && "$grove" land)
(cd ../gemini && "$grove" land)
(cd ../codex  && "$grove" land)
(cd ../cursor && "$grove" land)

say "A rename reaches a call site written after it"
(cd ../codex && "$grove" sync >/dev/null)
(cd ../dev && "$grove" sync >/dev/null)
python3 - ../codex/src/main.0 <<'PY' 2>/dev/null || python - ../codex/src/main.0 <<'PY'
import sys; p = sys.argv[1]; t = open(p).read(); open(p, "w").write(t.replace("parse(", "parseHeader("))
PY
import sys; p = sys.argv[1]; t = open(p).read(); open(p, "w").write(t.replace("parse(", "parseHeader("))
PY
(cd ../codex && "$grove" new "Rename parse to parseHeader")
edit ../dev/src/main.0 "return retryDelay(times) + 1" "return retryDelay(parse(times)) + 1"
(cd ../dev && "$grove" new "Parse retry counts")
(cd ../dev && "$grove" land)
(cd ../codex && "$grove" land)
"$grove" cat src/main.0 | grep -n "parse"

say "Two people change the same line: a saved conflict, main untouched"
(cd ../meera && "$grove" sync >/dev/null)
(cd ../claude && "$grove" sync >/dev/null)
edit ../meera/src/main.0  "return 8" "return 9"
edit ../claude/src/main.0 "return 8" "return 10"
(cd ../meera  && "$grove" new "Three items")
(cd ../claude && "$grove" new "Four items")
(cd ../meera  && "$grove" land)
(cd ../claude && "$grove" land) || true
"$grove" conflicts

say "History, provenance, undo"
"$grove" log
"$grove" why parseHeader --all
"$grove" undo
"$grove" log -n 2
"$grove" ops -n 4
"$grove" verify
