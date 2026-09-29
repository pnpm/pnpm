---
id: link
title: pnpm link
---

Aliases: `ln`

Links a local package to the current project's `node_modules`.

```text
pnpm link <dir>
```

## Options

### `pnpm link <dir>`

Links package from `<dir>` directory to `node_modules` of package from where you're executing this command. `<dir>` must be a relative or absolute path.

> For example, if you are inside `~/projects/foo` and you execute `pnpm link ../bar`, then a link to `bar` will be created in `foo/node_modules/bar`.

:::note Breaking changes in v11

`pnpm link` no longer resolves packages from the global store. Only relative or absolute paths are accepted (use `pnpm link ./foo` instead of `pnpm link foo`).

`pnpm link --global` has been removed. To register a local package's bins globally, use `pnpm add -g .` instead.

`pnpm link` with no arguments has been removed. Always pass an explicit path.

:::

## Use Cases

### Replace an installed package with a local version of it

Let's say you have a project that uses `foo` package. You want to make changes to `foo` and test them in your project. In this scenario, you can use `pnpm link` to link the local version of `foo` to your project:

```bash
cd ~/projects/foo
pnpm install # install dependencies of foo
cd ~/projects/my-project
pnpm link ~/projects/foo # link foo to my-project
```

### Add a binary globally

To make a local package's binaries available system-wide, use `pnpm add -g .` instead:

```bash
cd ~/projects/foo
pnpm install # install dependencies of foo
pnpm add -g . # register foo's bins globally
```

Remember that the binary will be available only if the package has a `bin` field in its `package.json`.

## What's the difference between `pnpm link` and using the `file:` protocol?

When you use `pnpm link`, the linked package is symlinked from the source code. You can modify the source code of the linked package, and the changes will be reflected in your project. With this method pnpm will not install the dependencies of the linked package, you will have to install them manually in the source code. This may be useful when you have to use a specific package manager for the linked package, for example, if you want to use `npm` for the linked package, but pnpm for your project.

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
