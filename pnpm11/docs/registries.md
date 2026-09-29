---
id: registries
title: Registries
---

Added in: v11.23.0

The [`registries`](./settings/dependency-resolution.md#registries) setting in `pnpm-workspace.yaml` declares each registry a project installs from — once, keyed by the registry's URL. Everything pnpm knows about a registry lives in its entry: the scopes routed to it, the bare-specifier prefix it answers to, and what the server behind it is like.

```yaml title="pnpm-workspace.yaml"
registries:
  https://npm.corp.example.com/:
    serverType: artifactory
    scopes: ["@acme", "@corp-tools"]
    prefix: work
  https://verdaccio.corp.example.com/:
    scopes: ["@"]
    supportsTimeField: true
```

The URL is the key because every fact in an entry is a fact about that server. If the tarball layout were keyed by scope instead, it would be bound to whoever the scope currently points at, and two developers whose scope resolves differently would write lockfiles that disagree about which URLs may be omitted.

An entry may carry:

| Field                                     | Type       | What it declares                                                     |
|-------------------------------------------|------------|----------------------------------------------------------------------|
| [`scopes`](#scopes)                       | `string[]` | The package scopes routed to this registry.                          |
| [`prefix`](#prefix)                       | `string`   | The bare-specifier prefix this registry answers to.                  |
| [`serverType`](#servertype)               | `string`   | How the server lays out tarball URLs: `npm` or `artifactory`.        |
| [`supportsTimeField`](#supportstimefield) | `boolean`  | Whether the server's abbreviated metadata carries the `time` field.  |

Any other field is rejected. In particular, credentials (`_authToken`, `_auth`, `_password`, `username`, `tokenHelper`) and TLS material (`ca`, `cafile`, `cert`, `certfile`, `key`, `keyfile`) are refused rather than silently ignored — `pnpm-workspace.yaml` is committed to the repository, so they belong in [`.npmrc`](./npmrc.md) (e.g. `//npm.corp.example.com/:_authToken=...`). A URL key that embeds `user:pass@` credentials is refused for the same reason.

## Routing packages to a registry

### scopes

The scopes whose packages resolve from this registry, `@`-prefixed. A bare `"@"` routes the scope-less default registry — the same registry the `registry` setting names:

```yaml title="pnpm-workspace.yaml"
registries:
  https://npm.corp.example.com/:
    scopes: ["@acme"]
  https://registry.internal.example.com/:
    scopes: ["@"]
```

A scope resolves to exactly one registry, so routing the same scope from two entries is an error.

### prefix

The [named-registry](./package-sources.md#named-registries) prefix this registry answers to, as in `"lib": "work:^2.0.0"`:

```yaml title="pnpm-workspace.yaml"
registries:
  https://npm.work.example.com/:
    prefix: work
```

```sh
pnpm add work:@corp/lib@^2.0.0
```

Each entry declares at most one prefix, and two entries cannot declare the same one: the prefix is the registry's identity in a lockfile package key (`foo@work:1.0.0`), so a second spelling would key the same package from the same registry twice.

Prefixes declared here follow the same rules as the deprecated [`namedRegistries`](./settings/dependency-resolution.md#namedregistries) setting they replace: the built-in `gh:` and `npmjs:` aliases can be overridden by declaring their prefix on your own URL, [reserved names](./settings/dependency-resolution.md#reserved-alias-names) are rejected, and packages resolved through a prefix get [registry-qualified lockfile keys](./settings/dependency-resolution.md#named-registries-in-the-lockfile).

## Describing the server

### serverType

Registries do not all lay out tarball URLs the way the npm registry does. pnpm omits a tarball URL from `pnpm-lock.yaml` when it can rebuild the URL from the package's name, version, and registry — the lockfile then carries no host-specific URL for that package, and renaming or moving the registry does not churn the lockfile. Whether pnpm can rebuild a URL depends on the server, and `serverType` is where you declare it. It has three states:

| `serverType`  | Meaning                                                                                                                                                                                                                                                                       |
|---------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| *undeclared*  | Strict: a tarball URL is omitted only when it is exactly the canonical npm-layout URL (`<registry>/<name>/-/<scopeless-name>-<version>.tgz`). This is how every registry but `registry.npmjs.org` is read by default.                                                          |
| `npm`         | The server behaves like `registry.npmjs.org`, which serves a scoped package from the percent-encoded path (`/@acme%2Fwidget/...`) as well as the unencoded one. A faithful mirror or caching proxy of the public registry is this. Built in for `registry.npmjs.org` itself.   |
| `artifactory` | The server repeats the scope in a scoped package's tarball filename: `@acme/widget/-/@acme/widget-1.0.0.tgz`, where the npm layout strips it (`@acme/widget/-/widget-1.0.0.tgz`).                                                                                              |

For a JFrog Artifactory registry this is the difference between a lockfile that records a host-specific tarball URL for **every scoped package** and one that records none of them:

```yaml title="pnpm-workspace.yaml"
registries:
  https://acme.jfrog.example.com/artifactory/api/npm/npm-virtual/:
    serverType: artifactory
    scopes: ["@acme"]
```

Values for common servers:

| Server                                                | `serverType`                                                                                                              |
|-------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------|
| JFrog Artifactory                                     | `artifactory`                                                                                                              |
| GitLab package registry                               | `artifactory` — it uses the same layout, repeating the scope in the filename.                                              |
| A faithful mirror or caching proxy of the npm registry | `npm`                                                                                                                      |
| Verdaccio, Sonatype Nexus, Azure Artifacts            | None needed — they serve the exact canonical npm layout, which the strict default already reconstructs.                    |
| GitHub Packages                                       | None — its download URLs contain a content digest that cannot be derived from the package's identity, so they are kept in the lockfile. |
| GitHub Enterprise Server                              | None — it serves a scoped package only from the percent-encoded path, which the strict default keeps in the lockfile and requests verbatim.  |

The layout is declared, never inferred: a virtual repository can serve both layouts at once, depending on whether each package was synced from an upstream or published locally, so there is no registry-level signal pnpm could sniff. Declaring it also keeps a wrong value a fixable misconfiguration instead of a silent breakage.

:::caution

`serverType` is a claim about the server that only its operator can make, and the lockfile depends on it: an omitted tarball URL is rebuilt using the declared layout on every install, including `--frozen-lockfile` installs. Declare a layout the server actually serves, and expect a one-time lockfile diff after declaring one — URLs that pnpm can now rebuild disappear from the lockfile on the next install that writes it.

:::

### supportsTimeField

Whether this registry's abbreviated metadata carries the `time` field. `registry.npmjs.org`'s does not, which is why the default is `false` and why time-based resolution features such as [`minimumReleaseAge`](./settings/dependency-resolution.md#minimumreleaseage) fall back to the far larger full metadata documents. A registry that does carry it — Verdaccio v5.15.1+ and several proxies — is worth declaring:

```yaml title="pnpm-workspace.yaml"
registries:
  https://verdaccio.corp.example.com/:
    scopes: ["@"]
    supportsTimeField: true
```

This is the per-registry form of the [`registrySupportsTimeField`](./settings/other.md#registrysupportstimefield) setting. The fallback is decided per registry: one registry that needs full metadata no longer costs it at the others, and `registrySupportsTimeField` remains the answer for every registry the project does not describe.

## Where the setting may live

The setting belongs in `pnpm-workspace.yaml` rather than `.npmrc` or the global config because the lockfile depends on it: one developer omitting tarball URLs that another reconstructs differently would break a frozen install.

The [global configuration file](./cli/config.md) (`config.yaml`) may declare the *routes* — `scopes` and `prefix` — so that a scope or an alias like `work:` applies to every project on the machine. The server descriptions (`serverType` and `supportsTimeField`) are read only from `pnpm-workspace.yaml`, for the reason above.

An entry that declares no routes describes a registry configured elsewhere — for example, the default registry set in `.npmrc`. Such an entry only takes effect when its URL is one the project actually resolves from; pnpm warns about entries that match no configured registry, since they would otherwise sit there inert (a stale URL, a scope that moved).

Environment variables are **not** expanded in the URL keys of this setting, for the same reason they are not expanded in other [registry URLs in `pnpm-workspace.yaml`](./settings.md): the file is committed, and expanding env variables into a request destination could leak secrets to an attacker-controlled host. A key containing a `${...}` placeholder is ignored.

## The older shapes

Before v11.23.0, `registries` mapped scopes to URLs, with the `default` key naming the main registry:

```yaml title="pnpm-workspace.yaml"
registries:
  default: https://registry.npmjs.org/
  "@acme": https://npm.corp.example.com/
```

This shape is still accepted and behaves as before. The two shapes cannot be mixed in one map — an entry whose value is a string is a scope route, and a scope route keyed by a URL is an error.

The [`namedRegistries`](./settings/dependency-resolution.md#namedregistries) setting is deprecated in favor of `prefix`. It is still read, but only for prefixes that `registries` does not declare; when both declare prefixes, pnpm warns.
