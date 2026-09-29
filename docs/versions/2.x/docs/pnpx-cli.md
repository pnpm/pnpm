---
id: pnpx-cli
title: pnpx CLI
original_id: pnpx-cli
---

npm has a great package runner called [npx](https://medium.com/@maybekatz/introducing-npx-an-npm-package-runner-55f7d4bd282b).
pnpm offers the same tool via the `pnpx` command. The only difference is that `pnpx` uses pnpm for installing packages.

The following command installs a temporary create-react-app and calls it,
without polluting global installs or requiring more than one step!

```
pnpx create-react-app my-cool-new-app
```
