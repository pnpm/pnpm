---
id: prune
title: pnpm prune
---

Removes unnecessary packages.

## Options

### --prod

Remove the packages specified in `devDependencies`.

### --no-optional

Remove the packages specified in `optionalDependencies`.

### --recursive, -r

Run `pnpm prune` in every package found in subdirectories, or in every workspace package when executed inside a workspace.

### --filter &lt;package_selector&gt;

Restrict `pnpm prune` to the selected workspace projects. [Read more about filtering.](../filtering.md)
