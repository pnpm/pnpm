---
id: npmrc
title: "Authentication Settings"
---

The settings on this page contain sensitive credentials and are stored in INI-formatted files. Do not commit these files to your repository.

For non-sensitive settings (proxy, SSL, registries, etc.), see [Settings (pnpm-workspace.yaml)](./settings.md).

## Auth file locations

pnpm reads authentication settings from the following files, in order of priority (highest first):

1. **`<workspace root>/.npmrc`** — project-level auth. This file should be listed in `.gitignore`.
2. **`<pnpm config>/auth.ini`** — the primary user-level auth file. `pnpm login` writes tokens here.
3. **`~/.npmrc`** — read as a fallback for easier migration from npm. Use the [`npmrcAuthFile`](./settings/other.md#npmrcauthfile) setting to point to a different file.

The `<pnpm config>` directory is:

* If the **$XDG_CONFIG_HOME** env variable is set: **$XDG_CONFIG_HOME/pnpm/**
* On Windows: **~/AppData/Local/pnpm/config/**
* On macOS: **~/Library/Preferences/pnpm/**
* On Linux: **~/.config/pnpm/**

## Environment variables in auth settings

Values in the **user-level** auth files (`<pnpm config>/auth.ini` and the user `.npmrc`) may reference environment variables using the `${NAME}` syntax:

```ini
//registry.npmjs.org/:_authToken=${NPM_TOKEN}
```

Since v11.28.0, npm's `${NAME?}` form is supported too. It expands to the value of `NAME`, or to an empty string without a warning when `NAME` is unset. pnpm warns when an empty environment variable removes an auth token, and the warning names the affected key.

Since v11.5.3, environment variables are **not** expanded in the **project-level** `.npmrc` at the workspace root for the following settings:

* registry and proxy URLs (`registry`, `@scope:registry`, proxy settings);
* URL-scoped keys (keys starting with `//`);
* credential values (`_authToken`, `_auth`, `_password`, `username`, `tokenHelper`, `cert`, `key`).

A setting that contains a `${...}` placeholder in any of these positions is ignored, and pnpm prints a warning. The project `.npmrc` is checked out together with the repository, so expanding environment variables there would allow a malicious repository to exfiltrate secrets from your environment (such as CI tokens) to an attacker-controlled registry during installation ([GHSA-3qhv-2rgh-x77r](https://github.com/pnpm/pnpm/security/advisories/GHSA-3qhv-2rgh-x77r)).

If your project relied on a committed `.npmrc` containing a line like `//registry.npmjs.org/:_authToken=${NPM_TOKEN}`, move the token to a trusted location instead:

* Write the token to the user-level auth file before installing (for example, in a CI step):

  ```sh
  pnpm config set //registry.npmjs.org/:_authToken "$NPM_TOKEN"
  ```

  `pnpm config set` writes to the global location by default (`<pnpm config>/auth.ini` for auth settings), not to the project `.npmrc`, so the token never ends up in the repository.

* **Set the credential through an environment variable, with no `.npmrc` file at all** (since v11.6). pnpm reads URL-scoped registry settings from `pnpm_config_//…` environment variables:

  ```sh
  env "pnpm_config_//registry.npmjs.org/:_authToken=$NPM_TOKEN" pnpm install
  ```

  The variable name contains `/`, `:`, and `.`, which `export` and the `NAME=value` shell assignment syntax reject as invalid identifiers. Use the `env` utility (as shown above) to pass it to a single command, or set it through a tool that accepts arbitrary variable names (for example, your CI provider's environment settings or Node's `process.env`).

  This is the most direct, file-free replacement for a committed `//registry.npmjs.org/:_authToken=${NPM_TOKEN}` line. Because the registry the credential applies to is encoded in the (trusted) variable name, a malicious repository cannot redirect it to another host. Such an environment value overrides the project `.npmrc` but is itself overridden by a command-line option. The `tokenHelper` setting is intentionally not read from environment variables.

* Or keep the `${NPM_TOKEN}` placeholder line, but put it in the user-level `~/.npmrc` (or the file referenced by [`npmrcAuthFile`](./settings/other.md#npmrcauthfile)) instead of the repository.
* In GitHub Actions, `actions/setup-node` with the `registry-url` input writes the auth setting to a user-level `.npmrc` (referenced by the `NPM_CONFIG_USERCONFIG` environment variable, which pnpm honors), so authentication via the `NODE_AUTH_TOKEN` environment variable continues to work.
* If you cannot easily modify each CI pipeline, you may declare the project `.npmrc` trusted by setting a single environment variable in the CI environment (for example, at the organization or workspace level):

  ```text
  PNPM_CONFIG_NPMRC_AUTH_FILE=.npmrc
  ```

  This is the env form of the [`npmrcAuthFile`](./settings/other.md#npmrcauthfile) setting: it makes pnpm read the project's `.npmrc` as the user-level auth file (a relative path is resolved against the working directory), so environment variables in it are expanded as before. Because the trust declaration comes from the environment — not from the repository — a malicious repository cannot set it for you. The npm-style `NPM_CONFIG_USERCONFIG` variable is also honored as a fallback.

  :::danger

  Only use this in environments that exclusively build trusted repositories. It disables this protection entirely for the checked-out repository, including the restriction that `tokenHelper` may only be set in user-level config.

  :::

The same rule applies to **registry and proxy URLs** in a project `.npmrc` (`registry`, `@scope:registry`, `proxy`, `https-proxy`, `http-proxy`). If you used an environment variable to build a registry URL, move the setting to a trusted source — your user-level `~/.npmrc`, or `pnpm config set "<key>" <value>`. If the URL is not secret, you can also write the resolved value directly in the project `.npmrc`, since only `${...}` placeholders are ignored. For registry settings in `pnpm-workspace.yaml`, see [Settings](./settings/dependency-resolution.md#registries).

## Authentication Settings

### &lt;URL&gt;&#58;_authToken

Define the authentication bearer token to use when accessing the specified
registry. For example:

```ini
//registry.npmjs.org/:_authToken=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

You may also use an environment variable. For example:

```ini
//registry.npmjs.org/:_authToken=${NPM_TOKEN}
```

Environment variables are only expanded in user-level auth files, not in the project-level `.npmrc`. See [Environment variables in auth settings](#environment-variables-in-auth-settings).

#### Scope-specific auth tokens

Added in: v11.7.0

pnpm can use different auth tokens for different package scopes, even when those scopes point to the same registry URL. Add the package scope after the registry URL in the auth key:

```ini
@org-a:registry=https://npm.pkg.github.com/
@org-b:registry=https://npm.pkg.github.com/

//npm.pkg.github.com/:@org-a:_authToken=ORG_A_TOKEN
//npm.pkg.github.com/:@org-b:_authToken=ORG_B_TOKEN

//npm.pkg.github.com/:_authToken=FALLBACK_TOKEN
```

When installing or publishing `@org-a/*`, pnpm uses `ORG_A_TOKEN`; for `@org-b/*`, it uses `ORG_B_TOKEN`. Optionally, packages without a matching scope fall back to the registry-wide token (`FALLBACK_TOKEN` above), when provided.

`pnpm login --registry=https://npm.pkg.github.com --scope=@org-a` writes the token to the same scope-specific auth key.

This is useful for registries (such as GitHub Packages) that issue tokens per organization or per scope. Previously, auth was selected only by registry URL, so two scopes sharing a registry had to share a token.

### &lt;URL&gt;&#58;tokenHelper

A token helper is an executable which outputs an auth token. This can be used in situations where the authToken is not a constant value but is something that refreshes regularly, where a script or other tool can use an existing refresh token to obtain a new access token.

The configuration for the path to the helper must be an absolute path, with no arguments. In order to be secure, it is only permitted to set this value in the user `.npmrc`. Otherwise a project could place a value in a project's local `.npmrc` and run arbitrary executables.

Setting a token helper for the default registry:

```ini
tokenHelper=/home/ivan/token-generator
```

Setting a token helper for the specified registry:

```
//registry.corp.com:tokenHelper=/home/ivan/token-generator
```

### _auth

Added in: v11.10.0

Configures registry authentication as a single structured value, keyed by registry URL. This is an alternative to the many `//host/:_authToken=…` entries and is designed for CI, where the URL-scoped form (whose variable name contains `/`, `:`, and `.`) cannot be passed through an environment variable on some runners.

`_auth` is honored **only** from two trusted locations:

* the **global** pnpm config (`config.yaml`);
* the `pnpm_config__auth` environment variable (for CI).

It is **ignored** in a project `pnpm-workspace.yaml` or `.npmrc`, so a checked-out repository can never supply registry auth.

The value is keyed by registry URL, so each secret is explicitly bound to the host that may receive it. Registry URL keys must use `http` or `https` and must not include credentials, query strings, or fragments. Within each registry URL, `@` means registry-wide (default) credentials, and a package scope such as `@org` binds credentials to that scope on the same host. The only supported credential field is `authToken` (it maps to `_authToken` / bearer auth); the deprecated `basicAuth` / `username` + `password` forms and `tokenHelper` are not accepted here.

In the global `config.yaml`:

```yaml
_auth:
  https://registry.npmjs.org:
    "@":
      authToken: npm-token
    "@org":
      authToken: org-token
```

The equivalent environment variable (a JSON string):

```sh
export pnpm_config__auth='{"https://registry.npmjs.org":{"@":{"authToken":"npm-token"},"@org":{"authToken":"org-token"}}}'
```

Both `pnpm_config__auth` (lowercase) and `PNPM_CONFIG__AUTH` (all-caps, the convention some CI runners apply) are honored. If both are set, lowercase wins unless it is empty, in which case uppercase is used.

Each entry also infers a trusted registry route: `@` routes the default registry (and `pnpm add <pkg>` resolves there), and `@org` routes that scope. Because the credential and its destination host arrive in one trusted value, repo-controlled config cannot redirect the token to a different host.

Precedence, from highest to lowest:

1. CLI flags (`--registry`, `--@scope:registry`)
2. `pnpm_config__auth` / `PNPM_CONFIG__AUTH`
3. global `config.yaml` `_auth`
4. `pnpm-workspace.yaml`

Parsing is strict: a malformed value (bad JSON, wrong shape, an invalid registry URL or scope, or an unsupported credential field) fails fast with an error rather than being silently dropped.

Since v11.28.0, environment variable placeholders such as `${NPM_TOKEN}` in an `authToken` value are expanded in both locations.

## Certificate Settings

### ca

* Default: **The npm CA certificate**
* Type: **String, Array or null**

The Certificate Authority signing certificate that is trusted for SSL
connections to the registry. Values should be in PEM format (AKA
"Base-64 encoded X.509 (.CER)"). For example:

```sh
ca="-----BEGIN CERTIFICATE-----\nXXXX\nXXXX\n-----END CERTIFICATE-----"
```

Set to null to only allow known registrars, or to a specific CA cert to trust
only that specific signing authority.

Multiple CAs can be trusted by specifying an array of certificates:

```sh
ca[]="..."
ca[]="..."
```

See also the [`strictSsl`](./settings/network.md#strictssl) setting.

### cafile

* Default: **null**
* Type: **path**

A path to a file containing one or multiple Certificate Authority signing
certificates. Similar to the `ca` setting, but allows for multiple CAs, as well
as for the CA information to be stored in a file instead of being specified via
CLI.

### &lt;URL&gt;&#58;cafile

Define the path to a Certificate Authority file to use when accessing the specified
registry. For example:

```sh
//registry.npmjs.org/:cafile=ca-cert.pem
```

### &lt;URL&gt;&#58;ca

Added in: v10.25.0

Define an inline Certificate Authority certificate for the specified registry.
The value must be PEM-encoded, like the global `ca` setting, but it only applies
to the matching registry URL.

```sh
//registry.example.com/:ca=-----BEGIN CERTIFICATE-----...-----END CERTIFICATE-----
```

### cert

* Default: **null**
* Type: **String**

A client certificate to pass when accessing the registry. Values should be in
PEM format (AKA "Base-64 encoded X.509 (.CER)"). For example:

```test
cert="-----BEGIN CERTIFICATE-----\nXXXX\nXXXX\n-----END CERTIFICATE-----"
```

It is not the path to a certificate file.

### &lt;URL&gt;&#58;cert

Added in: v10.25.0

Define an inline client certificate to use when accessing the specified
registry. Example:

```sh
//registry.example.com/:cert=-----BEGIN CERTIFICATE-----...-----END CERTIFICATE-----
```

### &lt;URL&gt;&#58;certfile

Define the path to a certificate file to use when accessing the specified
registry. For example:

```sh
//registry.npmjs.org/:certfile=server-cert.pem
```

### key

* Default: **null**
* Type: **String**

A client key to pass when accessing the registry. Values should be in PEM format
(AKA "Base-64 encoded X.509 (.CER)"). For example:

```sh
key="-----BEGIN PRIVATE KEY-----\nXXXX\nXXXX\n-----END PRIVATE KEY-----"
```

It is not the path to a key file. Use `<URL>&#58;keyfile` if you need to reference
the file system instead of inlining the key.

This setting contains sensitive information. Don't write it to a local `.npmrc` file committed to the repository.

### &lt;URL&gt;&#58;key

Added in: v10.25.0

Define an inline client key for the specified registry URL.

```sh
//registry.example.com/:key=-----BEGIN PRIVATE KEY-----...-----END PRIVATE KEY-----
```

### &lt;URL&gt;&#58;keyfile

Define the path to a client key file to use when accessing the specified
registry. For example:

```sh
//registry.npmjs.org/:keyfile=server-key.pem
```
