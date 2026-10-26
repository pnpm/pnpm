---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

With `nodeLinker: hoisted`, an install that stops partway no longer leaves the next install trusting the packages it was replacing. Before, reverting the change that started the stopped install, such as a patch edit, kept the half-replaced package in `node_modules`.
