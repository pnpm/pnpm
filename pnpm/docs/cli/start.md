---
id: start
title: pnpm start
---

Equivalent to `pnpm run start`.

```sh
pnpm start [<args>...]
```

Runs an arbitrary command specified in the package's `start` property of its
`scripts` object. Arguments after `pnpm start` are passed to the script verbatim.

If no `start` property is specified on the `scripts` object, pnpm runs
`node server.js` when `server.js` exists in the project directory. The
`prestart` and `poststart` scripts still run around it when
[`enablePrePostScripts`](../settings/other.md#enableprepostscripts) is on. If
neither exists, the command fails with `Missing script start or file server.js`.

The intended usage of the property is to specify a command that starts your
program.
