#!/usr/bin/env bash
# Repository hygiene: fails if assistant state, credentials or assistant
# attribution got into tracked files or into commits not yet on origin/main.
set -euo pipefail
cd "$(dirname "$0")/../.."
bad=0
paths=$(git ls-files | grep -E '(^|/)(\.claude/|CLAUDE(\.local)?\.md$|\.mcp\.json$|\.env(\..*)?$|.*\.(pem|key|p12|pfx)$|id_(rsa|ed25519))' || true)
[ -n "$paths" ] && { echo "hygiene: tracked files that must not be in git:"; echo "$paths"; bad=1; }
text=$(git grep -nIE 'claude\.ai/|noreply@anthropic\.com|Co-Authored-By:' -- . ':!tools/gates/hygiene.sh' ':!tools/hooks/' || true)
[ -n "$text" ] && { echo "hygiene: assistant links or trailers in tracked files:"; echo "$text"; bad=1; }
base=$(git merge-base HEAD origin/main 2>/dev/null || true)
if [ -n "$base" ]; then
    msgs=$(git log --format='%h %an <%ae>%n%B' "$base"..HEAD | grep -iE '^(co-authored-by|generated[ -]with|generated-by):|noreply@anthropic\.com|claude\.ai/'  || true)
    [ -n "$msgs" ] && { echo "hygiene: unpushed commits carry assistant attribution:"; echo "$msgs"; bad=1; }
fi
exit $bad
