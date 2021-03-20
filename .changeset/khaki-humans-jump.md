---
"@pnpm/lifecycle": patch
---

Run `node-gyp` when `binding.gyp` is present, even if an install lifecycle script is not present in the scripts field.
