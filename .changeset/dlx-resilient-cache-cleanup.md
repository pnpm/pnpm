---
"@pnpm/exec.commands": patch
"pnpm": patch
---

Fixed a Windows flakiness in `pnpm dlx` where a failed install could surface a spurious `EBUSY: resource busy or locked` error. The cleanup of a partially-populated dlx cache is now best-effort with retries and no longer masks the original error.
