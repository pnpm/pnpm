---
"@pnpm/pnpr": patch
---

pnpr now resolves JSR packages without configuration. npm.jsr.io is a built-in public route, alongside the npm registry, so a client whose graph holds a JSR dependency no longer needs the operator to declare that origin.
