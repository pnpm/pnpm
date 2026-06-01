---
"@pnpm/deps.compliance.audit": patch
"pnpm": patch
---

Improve `pnpm audit` performance by pruning non-vulnerable lockfile subtrees and stopping path enumeration once vulnerable findings reach the path cap.
