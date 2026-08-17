---
"pacquet": patch
---

`pnpm install` and `pnpm dedupe` no longer eat all the available memory while resolving a graph in which many packages declare the same missing peer dependency, such as the `react` peer the `@radix-ui` packages share [#13786](https://github.com/pnpm/pnpm/issues/13786).
