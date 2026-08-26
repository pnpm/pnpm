---
"pacquet": patch
---

Fixed Plug'n'Play projects to preload `.pnp.cjs` for dependency and project lifecycle scripts, `pnpm run`, and `pnpm exec`. The generated loader now also exposes the public Yarn PnP API surface.
