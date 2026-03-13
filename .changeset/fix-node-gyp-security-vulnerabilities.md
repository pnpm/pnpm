---
"pnpm": patch
---

Upgrade bundled `node-gyp` from `^11.5.0` to `^12.2.0` to fix multiple security vulnerabilities in transitive dependencies bundled into `dist/node_modules`. Add security overrides in `pnpm-workspace.yaml` to enforce minimum secure versions of transitive dependencies: tar >=7.5.11, tough-cookie >=4.1.3, minimist >=1.2.6, y18n (>=3.2.2 for 3.x, >=4.0.1 for 4.x, >=5.0.5 for 5.x), and json-schema >=0.4.0. Version-line-specific overrides are used for y18n to avoid unnecessary major version jumps.
