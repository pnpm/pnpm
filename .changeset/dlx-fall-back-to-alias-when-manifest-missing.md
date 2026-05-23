---
"@pnpm/exec.commands": patch
pnpm: patch
---

Fixed `pnpm dlx` failing with `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND` when the installed package's CAS slot is missing its `package.json`. Observed in the wild for `pnpm dlx node@runtime:<version>` when the GVS slot was populated without the synthesized manifest runtime archives need (they don't ship a `package.json` of their own, so the synthesized one is the only way it gets there; an existing slot from an earlier code path that skipped the synthesis stays incomplete). The bin link itself is wired up from the resolution and remains valid, so `dlx` now falls back to the scopeless package name when the slot's manifest is unreadable — for single-bin packages (the dlx common case, including every `runtime:` spec) this matches what `manifest.bin` would have named. Multi-bin packages already require `--package=<spec> <bin>` to disambiguate and don't enter this code path.
