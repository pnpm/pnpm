---
id: why
title: pnpm why
---

Shows all packages that depend on the specified package.

:::caution

If the Dependencies Tree has more than 10 results (end leaves), the output will be truncated to 10 end leaves.

This makes the output more readable and avoids memory issues.

:::

## Options

### --recursive, -r

Show the dependency tree for the specified package on every package in
subdirectories or on every workspace package when executed inside a workspace.

### --json

Show information in JSON format.

### --long

Show verbose output.

### --parseable

Show parseable output instead of tree view.

### --global, -g

List packages in the global install directory instead of in the current project.

### --prod, -P

Only display the dependency tree for packages in `dependencies`.

### --dev, -D

Only display the dependency tree for packages in `devDependencies`.

### --depth &lt;number\>

Display only dependencies within a specific depth.

### --only-projects

Display only dependencies that are also projects within the workspace.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
