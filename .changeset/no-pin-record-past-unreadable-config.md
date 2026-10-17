---
"pnpm": patch
---

pnpm no longer writes the pinned pnpm version to `pnpm-lock.yaml` when it cannot read every setting and the running pnpm already matches the pin.
