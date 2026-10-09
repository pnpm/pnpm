---
id: web-ui
title: Web UI
---

Added in: v0.1.0-alpha.15

pnpr serves a web UI at `/-/ui/` for browsing its registries and packages. The
UI is the separate `@pnpm/pnpr-ui` package. Install it next to pnpr:

```sh
pnpm add -g @pnpm/pnpr,@pnpm/pnpr-ui
```

The comma puts both packages in one
[global install group](https://pnpm.io/global-packages#isolated-installations),
where pnpr can find the UI. With npm, run
`npm install -g @pnpm/pnpr @pnpm/pnpr-ui`.

Start pnpr and open `http://127.0.0.1:7677/-/ui/`. The UI talks to the server
that serves it, so it needs no [CORS](discovery.md) setting. To see private
packages, enter a token on the Connect screen. The UI keeps the token in memory
only, so a reload asks for it again.

pnpr serves the UI only when the
[registry surface](configuration.md#registry-resolver-and-artifact-surfaces) is
enabled. The UI loads its font from Google Fonts.

## Settings

```yaml title="pnpr.yaml"
ui:
  enabled: true
  dir: ./pnpr-ui
```

- `enabled`: `false` serves no UI, even when `@pnpm/pnpr-ui` is installed.
  Defaults to `true`.
- `dir`: the directory of a built UI, such as the `dist` directory of
  `@pnpm/pnpr-ui`. A relative path is resolved against the directory of the
  config file. pnpr does not start when the directory has no `index.html`.
  Without `dir`, pnpr serves `@pnpm/pnpr-ui` when it is installed next to pnpr.

The container image does not include the UI. To serve it from a container,
mount the `dist` directory of `@pnpm/pnpr-ui` and set `dir`.
