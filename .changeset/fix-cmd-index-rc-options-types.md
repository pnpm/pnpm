---
"pnpm": patch
---

Fixed variable shadowing bug in command registration that prevented rc options types from being properly collected. The `rcOptionsTypes` variable was being shadowed in the destructuring assignment, causing `Object.assign()` to incorrectly reference the outer Record instead of calling the command's rcOptionsTypes function.
