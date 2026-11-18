---
"@pnpm/patching.apply-patch": patch
"pnpm": patch
"pacquet": patch
---

Patches that add lines to the end of a file without a final newline now install the whole file. pnpm dropped the last added line when the patch also marked it with `\ No newline at end of file` [#18156](https://github.com/pnpm/pnpm/issues/18156).
