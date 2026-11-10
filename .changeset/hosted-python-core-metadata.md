---
"@pnpm/pnpr": patch
---

Hosted Python wheel uploads now advertise core metadata with its SHA-256 and serve it at `<filename>.metadata`. Installers can resolve dependencies without downloading the whole wheel.
