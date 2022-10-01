---
"pnpm": patch
---

fix: Stop --filter-prod option to run command on all the projects when used on workspace. --filter-prod option now only filter from `dependencies` and omit `devDependencies` instead of including all the packages when used on workspace. So what was happening is that if you use `--filter-prod` on workspace root like this:
```bash
pnpm --filter-prod ...build-modules exec node -e 'console.log(require(`./package.json`).name)'
```
it was printing all the package of workspace, where it should only print the package name of itself and packages where it has been added as `dependency` (not as `devDependencies`)
