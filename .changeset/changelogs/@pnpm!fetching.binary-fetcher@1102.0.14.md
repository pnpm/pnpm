## 1102.0.14

### Patch Changes

- pnpm now unpacks a downloaded runtime archive into a randomly named directory inside the store. It previously used a predictable path, where another user of a shared store could plant a symlink and redirect the write outside the store ([GHSA-vwc7-r8mq-g2x9](https://github.com/advisories/GHSA-vwc7-r8mq-g2x9)).
