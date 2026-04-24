# @pnpm/resolving.named-registry-specifier-parser

> Parser of named-registry specifiers (e.g. `gh:@acme/pkg`)

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/resolving.named-registry-specifier-parser.svg)](https://www.npmjs.com/package/@pnpm/resolving.named-registry-specifier-parser)
<!--/@-->

Parses specifiers that target a registry referenced by a short alias instead of a full URL — for example `gh:@acme/pkg` for GitHub Packages, or a user-defined `work:@corp/lib` for a private registry.

The built-in `gh` alias maps to the [GitHub Packages](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry) npm registry. `github:` itself is reserved by npm-package-arg as a git host shorthand, so pnpm uses `gh:` instead.

Additional aliases — or an override for the built-in `gh` alias — can be configured via the `namedRegistries` setting in `pnpm-workspace.yaml`.

Supported syntaxes:

- `<alias>:@<owner>/<name>`
- `<alias>:@<owner>/<name>@<version_selector>`
- `<alias>:<version_selector>` (only when a scoped package alias is provided)

Unlike [JSR](https://jsr.io/), named registries do not rewrite the package name — the package name is preserved as is and only the registry URL changes.

## Installation

```sh
pnpm add @pnpm/resolving.named-registry-specifier-parser
```

## License

MIT
