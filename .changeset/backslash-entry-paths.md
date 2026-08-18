---
"pacquet": patch
---

Archive entries whose paths use `\` as a separator are now read the same way pnpm reads them. A nested path spelled `bin\tool.js` by Windows publishing tooling resolves to `bin/tool.js`, and a path traversal spelled with backslashes is rejected instead of being stored verbatim.
