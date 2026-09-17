---
"pacquet": minor
---

`pnpm install` can install a Python release that publishes no wheel this interpreter accepts, by building the source distribution the index serves beside it [#14945](https://github.com/pnpm/pnpm/issues/14945). The archive is pinned in `pylock.toml` by name and SHA-256. A later install replays it from the store, offline included. Building one runs the release's own build backend, so it needs `pkg:pypi/<distribution>: true` under `allowBuilds`.

A resolution that finds no version of a distribution now says why. It tells apart a distribution no index publishes, one whose releases publish nothing this interpreter can install, and one whose versions the project's requirements exclude.
