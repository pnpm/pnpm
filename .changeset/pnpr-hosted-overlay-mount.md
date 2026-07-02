---
"@pnpm/pnpr": minor
---

A hosted mount can now declare `mirror: <upstream-mount>` to act as an **overlay**: a read for a name the mount does not host is served (and cached) from the declared upstream, and a dist-tag or publish write that finds no hosted packument materializes the upstream's first. The backing is a single origin named in config and validated at load — not a router existence-based fall-through — so the mount model's declared-provenance rule still holds. The bundled registry-mock config now uses this shape (a single `local` mount mirrored on `npmjs`), which restores dist-tag and publish writes against packages proxied from npm and lets reads of any non-hosted name fall through to npm.
