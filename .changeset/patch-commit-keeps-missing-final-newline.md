---
"pnpm": patch
---

`pnpm patch-commit` now keeps a file without a final newline unterminated when that file comes last in the patch. Previously the patch dropped the final `\ No newline at end of file` marker, so the patched file gained a trailing newline.
