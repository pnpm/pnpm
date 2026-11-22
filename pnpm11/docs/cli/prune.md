---
id: prune
title: pnpm prune
---

Removes unnecessary packages.

With `nodeLinker: hoisted`, pnpm removes extraneous package links from the selected projects' `node_modules` directories. The link targets stay on disk.

## Options

### --prod

Remove the packages specified in `devDependencies`.

### --no-optional

Remove the packages specified in `optionalDependencies`.

### --recursive, -r

Run `pnpm prune` in every package found in subdirectories, or in every workspace package when executed inside a workspace.

### --filter &lt;package_selector&gt;

Restrict `pnpm prune` to the selected workspace projects. [Read more about filtering.](../filtering.md)
