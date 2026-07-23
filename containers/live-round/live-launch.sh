#!/bin/sh -eu
# Live-round LAUNCH live fire — with a WRITABLE ~/.claude.
# The credential is mounted read-only at /cred; we copy it into a container-owned ~/.claude so the
# claude CLI can mkdir its own jobs/sessions dirs (bind-mounting the file directly makes ~/.claude
# root-owned → EACCES on mkdir). Container-only; every spawned process dies with this --rm box.
set +e
LOG() { echo; echo "===== $* ====="; }

LOG "set up a writable ~/.claude from the read-only credential mount"
mkdir -p "$HOME/.claude"
cp /cred/.credentials.json "$HOME/.claude/.credentials.json"
chmod 600 "$HOME/.claude/.credentials.json"
ls -la "$HOME/.claude"

export QFS_CLAUDE_SESSIONS="$HOME/.claude"
export QFS_CLAUDE_BINARY="/home/agent/.local/bin/claude"
export CARGO_TARGET_DIR=/work/target
export CARGO_TERM_COLOR=never

LOG "sanity: claude --bg now starts (no EACCES)?"
claude --bg "Print the single word ok and then stop." ; echo "claude --bg rc=$?"
echo "-- sessions dir after a raw --bg --"
ls -la "$HOME/.claude/sessions" 2>&1 | head
claude agents --json 2>&1 | head -20

LOG "build the qfs binary (debug)"
cp -r /src/packages /work/pkg-packages 2>/dev/null
rm -rf /work/pkg-packages/qfs/.cargotmp 2>/dev/null
cd /work/pkg-packages/qfs || { echo "BLOCKED: no workspace"; exit 0; }
cargo build -p qfs --bin qfs 2>&1 | tail -4
QFS=/work/target/debug/qfs
[ -x "$QFS" ] || { echo "BLOCKED: qfs build produced no binary"; exit 0; }

LOG "baseline sessions read (should now show the raw --bg agent above)"
"$QFS" run '/hosts/local/claude/sessions |> select id, status |> limit 20' --format table 2>&1 | head -30

LOG "LEG C - launch via qfs: --commit --commit-irreversible (REAL claude --bg spawn)"
OUT=$("$QFS" run "insert into /hosts/local/claude/sessions values (cwd, prompt) ('/work','Print the single word ok and then stop.') returning id" --commit --commit-irreversible 2>&1)
echo "$OUT" | head -25
echo "qfs launch rc=$?"

LOG "LEG D - sessions relation after the qfs launch (new id present?)"
sleep 12
"$QFS" run '/hosts/local/claude/sessions |> select id, status, cwd |> limit 40' --format table 2>&1 | head -50
echo "-- raw claude agents --json --"
claude agents --json 2>&1 | head -30

LOG "DONE"
