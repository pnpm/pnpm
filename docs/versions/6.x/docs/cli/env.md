---
id: env
title: "pnpm env <cmd>"
---

Added in: v6.12.0

Manages the Node.js environment.

## Commands

### use

Install and use the specified version of Node.js

Install the LTS version of Node.js:

```
pnpm env use --global lts
pnpm env use --global argon
```

Install Node.js v16:

```
pnpm env use --global 16
```

Also since v6.18.0:

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

## Options

### --global, -g

The changes are made systemwide.
