---
id: link
title: pnpm link
---

Aliases: `ln`

Makes the current local package accessible system-wide, or in another location.

```text
pnpm link <dir>
pnpm link --global
pnpm link --global <pkg>
```

## Options

### --dir &lt;dir\>, -C

* **Default**: Current working directory
* **Type**: Path string

Changes the link location to `<dir>`.

### `pnpm link <dir>`

Links package from `<dir>` folder to node_modules of package from where you're executing this command or specified via `--dir` option.

> For example, if you are inside `~/projects/foo` and you execute `pnpm link --dir ../bar`, then `foo` will be linked to `bar/node_modules/foo`.

### `pnpm link --global`

Links package from location where this command was executed or specified via `--dir` option to global `node_modules`, so it can be referred from another package with `pnpm link --global <pkg>`. Also if the package has a `bin` field, then the package's binaries become available system-wide.

### `pnpm link --global <pkg>`

Links the specified package (`<pkg>`) from global `node_modules` to the `node_modules` of package from where this command was executed or specified via `--dir` option.

## Difference between `pnpm link <dir>` and `pnpm link --dir <dir>`

`pnpm link <dir>` links the package from `<dir>` to the `node_modules` of the package where the command was executed. `pnpm link --dir <dir>` links the package from the current working directory to `<dir>`.

```bash
# The current directory is foo
pnpm link ../bar

- foo
  - node_modules
    - bar -> ../../bar
- bar

# The current directory is bar
pnpm link --dir ../foo

- foo
  - node_modules
    - bar -> ../../bar
- bar
```

## Use Cases

### Replace an installed package with a local version of it

Let's say you have a project that uses `foo` package. You want to make changes to `foo` and test them in your project. In this scenario, you can use `pnpm link` to link the local version of `foo` to your project, while the `package.json` won't be modified.

```bash
cd ~/projects/foo
pnpm install # install dependencies of foo
pnpm link --global # link foo globally
cd ~/projects/my-project
pnpm link --global foo # link foo to my-project
```

You can also link a package from a directory to another directory, without using the global `node_modules` folder:

```bash
cd ~/projects/foo
pnpm install # install dependencies of foo
cd ~/projects/my-project
pnpm link ~/projects/foo # link foo to my-project
```

### Add a binary globally

If you are developing a package that has a binary, for example, a CLI tool, you can use `pnpm link --global` to make the binary available system-wide.
This is the same as using `pnpm install -g foo`, but it will use the local version of `foo` instead of downloading it from the registry.

Remember that the binary will be available only if the package has a `bin` field in its `package.json`.

```bash
cd ~/projects/foo
pnpm install # install dependencies of foo
pnpm link --global # link foo globally
```

## What's the difference between `pnpm link` and using the `file:` protocol?

When you use `pnpm link`, the linked package is symlinked from the source code. You can modify the source code of the linked package, and the changes will be reflected in your project. With this method pnpm will not install the dependencies of the linked package, you will have to install them manually in the source code. This may be usefull when you have to use a specific package manager for the linked package, for example, if you want to use `npm` for the linked package, but pnpm for your project.

When you use the `file:` protocol in `dependencies`, the linked package is hard-linked to your project `node_modules`, you can modify the source code of the linked package, and the changes will be reflected in your project. With this method pnpm will also install the dependencies of the linked package, overriding the `node_modules` of the linked package.

:::info

When dealing with **peer dependencies** it is recommended to use the `file:` protocol. It better resolves the peer dependencies from the project dependencies, ensuring that the linked dependency correctly uses the versions of the dependencies specified in your main project, leading to more consistent and expected behaviors.

:::

| Feature                                      | `pnpm link`                                        | `file:` Protocol                                    |
|----------------------------------------------|----------------------------------------------------|-----------------------------------------------------|
| Symlink/Hard-link                            | Symlink                                            | Hard-link                                           |
| Reflects source code modifications           | Yes                                                | Yes                                                 |
| Installs dependencies of the linked package  | No (manual installation required)                  | Yes (overrides `node_modules` of the linked package)|
| Use different package manager for dependency | Possible (e.g., use `npm` for linked pkg)          | No, it will use pnpm                                |
