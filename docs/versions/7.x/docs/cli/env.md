---
id: env
title: "pnpm env <cmd>"
---

Manages the Node.js environment.

<iframe width="560" height="315" src="https://www.youtube-nocookie.com/embed/84-MzN_0Cng" title="The pnpm patch command demo" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; fullscreen"></iframe>

## Commands

### use

Install and use the specified version of Node.js

Install the LTS version of Node.js:

```
pnpm env use --global lts
```

Install Node.js v16:

```
pnpm env use --global 16
```

Install a prerelease version of Node.js:

```
pnpm env use --global nightly
pnpm env use --global rc
pnpm env use --global 16.0.0-rc.0
pnpm env use --global rc/14
```

Install the latest version of Node.js:

```
pnpm env use --global latest
```

Install an LTS version of Node.js using its [codename]:

```
pnpm env use --global argon
```

[codename]: https://github.com/nodejs/Release/blob/main/CODENAMES.md

### remove, rm

Added in: v7.10.0

Removes the specified version of Node.JS.

Usage example:

```
pnpm env remove --global 14.0.0
```

### list, ls

Added in: v7.16.0

List Node.js versions available locally or remotely.

Print locally installed versions:

```
pnpm env list
```

Print remotely available Node.js versions:

```
pnpm env list --remote
```

Print remotely available Node.js v16 versions:

```
pnpm env list --remote 16
```

## Options

### --global, -g

The changes are made systemwide.
