---
name: git-commit-style
description: Never add Co-Authored-By lines or any Claude attribution to git commits
metadata:
  type: feedback
---

User does NOT want "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>" or any similar Claude attribution in git commit messages. Keep commits clean — just the subject, body, and optionally a blank line between them.

**Why:** The user wants their own name on commits. Claude is a tool, not a co-author.

**How to apply:** Never add `Co-Authored-By` lines to any commit message. If one was accidentally added, amend the commit to remove it.
