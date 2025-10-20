#!/bin/bash
# Stage all changes and create commit
git add -A && git commit -m "$(cat <<'EOF'
feat: add universal protection against infinite re-evaluation loops

Simplified the InfiniteReevaluationProtector to protect against ANY cause
of infinite re-evaluation loops, not just specific scenarios.

Changes:
- Replaced specialized flip-counting with universal total count tracking
- Set limit to 500 re-evaluations before stopping
- Counter resets when condition value stabilizes
- Protects against: flipping conditions, never-resolving conditions,
  circular dependencies, and any other infinite loop causes

Updated tests:
- Modified existing tests for new 500-iteration limit
- Added test for conditions that never resolve (always return None)
- All 44 tests pass

The implementation is simple, general, and effectively protects users
from infinite loops regardless of the underlying cause.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
