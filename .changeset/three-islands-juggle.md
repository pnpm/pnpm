---
"@pnpm/resolve-dependencies": patch
pnpm: patch
---

When using pnpm catalogs and running a normal `pnpm install`, pnpm produced false positive warnings for "_skip adding to the default catalog because it already exists_". This warning now only prints when using `pnpm add --save-catalog` as originally intended.
