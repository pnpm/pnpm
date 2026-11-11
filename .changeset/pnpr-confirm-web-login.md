---
"@pnpm/pnpr": patch
---

`pnpm login` against pnpr now asks for confirmation in the browser. pnpr shows a code there, and the user continues only if the terminal shows the same code.

A token that a passwordless account creates now expires no later than the session or token that created it. It also stays within that credential's CIDR whitelist.
