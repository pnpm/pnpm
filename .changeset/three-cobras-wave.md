---
"@pnpm/plugin-commands-script-runners": patch
---

A recursive run should not rerun the same package script which started the lifecycle event.

For instance, let's say one of the workspace projects has the following script:

```json
"scripts": {
  "build": "pnpm run -r build"
}
```

Running `pnpm run build` in this project should not start an infinite recursion.
`pnpm run -r build` in this case should run `build` in all the workspace projects except the one that started the build.

Related issue: #2528
