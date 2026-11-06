---
id: fund
title: pnpm fund
---

Lists funding information for installed dependencies, grouped by funding URL.

```sh
pnpm fund
pnpm fund --json
pnpm --recursive fund
pnpm --filter my-app fund
```

`pnpm fund <package>` opens the funding URL of the newest installed version of
that package. You can also pass a project path, such as `pnpm fund .`. Packages
that are not installed are not fetched from the registry.

If a package lists several funding URLs, pnpm prints a numbered list. Use
`pnpm fund <package> --which <number>` to open one. Numbers start at 1.

## Options

### --json

Print the funding report as JSON. Recursive and filtered runs print an array
of reports, one per selected project.

### --which &lt;number\>

Select the funding URL to open when a package lists several sources.
