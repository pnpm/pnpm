---
id: npmrc
title: "Authentication Settings"
---

This page covers registry credentials and client certificates. These settings
hold secrets. Keep them out of your repository.

For other settings, such as proxies and registries, see
[Settings (pnpm-workspace.yaml)](./settings.md).

## Quick start

On your machine, log in to the registry:

```sh
pnpm login
```

pnpm saves the token in `<pnpm config>/auth.ini`. See
[Auth file locations](#auth-file-locations).

In CI, pass the token through an environment variable:

```sh
export pnpm_config__auth='{"https://registry.npmjs.org":{"@":{"authToken":"${NPM_TOKEN}"}}}'
```

## Auth file locations

pnpm reads auth settings from two kinds of files.

INI files in npm's `.npmrc` format hold the
[`.npmrc` and `auth.ini` settings](#settings-in-npmrc-and-authini) and the
[certificate settings](#certificate-settings). If a key is set in more than one
file, the first file in this list wins:

1. `<workspace root>/.npmrc`, the project file. Add it to `.gitignore`.
2. `<pnpm config>/auth.ini`. `pnpm login` and `pnpm config set` write auth
   keys here.
3. `~/.npmrc`, so credentials you set up for npm keep working. To read a
   different file, use the [`npmrcAuthFile`](./settings/other.md#npmrcauthfile)
   setting.

The global `<pnpm config>/config.yaml` can hold the [`_auth`](#_auth)
setting.

The `<pnpm config>` directory is:

* `$XDG_CONFIG_HOME/pnpm/`, if `XDG_CONFIG_HOME` is set
* `~/AppData/Local/pnpm/config/` on Windows
* `~/Library/Preferences/pnpm/` on macOS
* `~/.config/pnpm/` on Linux

## Settings in `.npmrc` and `auth.ini`

Set these in one of the INI files listed in
[Auth file locations](#auth-file-locations).

### &lt;URL&gt;&#58;_authToken

The bearer token for a registry:

```ini
//registry.npmjs.org/:_authToken=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

In a user-level auth file, the value can reference an environment variable:

```ini
//registry.npmjs.org/:_authToken=${NPM_TOKEN}
```

See [Environment variables in auth settings](#environment-variables-in-auth-settings).

#### Scope-specific auth tokens

Some registries, such as GitHub Packages, issue a separate token for each
scope. Put the scope after the registry URL in the key:

```ini
@org-a:registry=https://npm.pkg.github.com/
@org-b:registry=https://npm.pkg.github.com/

//npm.pkg.github.com/:@org-a:_authToken=ORG_A_TOKEN
//npm.pkg.github.com/:@org-b:_authToken=ORG_B_TOKEN

//npm.pkg.github.com/:_authToken=FALLBACK_TOKEN
```

pnpm sends `ORG_A_TOKEN` for `@org-a/*` packages and `ORG_B_TOKEN` for
`@org-b/*` packages. Other packages from that registry get `FALLBACK_TOKEN`,
if it is set.

`pnpm login --registry=https://npm.pkg.github.com --scope=@org-a` saves the
token under the same scoped key.

### &lt;URL&gt;&#58;tokenHelper

The path to a program that prints a token. Use it for tokens that expire, so a
script can fetch a fresh one.

pnpm runs the program and sends its output as `Authorization: Bearer <output>`.
If the output already starts with a word and a space, such as `Bearer abc`,
`Basic abc`, or `Token abc`, pnpm sends it unchanged.

The path must be absolute and take no arguments. pnpm reads `tokenHelper` only
from user-level auth files. A project `.npmrc` could otherwise make pnpm run
any program. Environment variables cannot set it either.

For the default registry:

```ini
tokenHelper=/home/ivan/token-generator
```

For a specific registry:

```ini
//registry.corp.com:tokenHelper=/home/ivan/token-generator
```

## Settings in `config.yaml`

### _auth

All registry tokens in one value, keyed by registry URL. Use it in CI, where
the `//host/:_authToken` key cannot be an environment variable name on some
runners.

pnpm reads `_auth` from two places only:

* the global `config.yaml`
* the `pnpm_config__auth` environment variable, as JSON

pnpm ignores `_auth` in `pnpm-workspace.yaml` and `.npmrc`.

In the global `config.yaml`:

```yaml title="config.yaml"
_auth:
  https://registry.npmjs.org:
    "@":
      authToken: npm-token
    "@org":
      authToken: org-token
```

The same value as an environment variable:

```sh
export pnpm_config__auth='{"https://registry.npmjs.org":{"@":{"authToken":"npm-token"},"@org":{"authToken":"org-token"}}}'
```

pnpm also reads `PNPM_CONFIG__AUTH`, the all-caps name some CI runners use. If
both are set, the lowercase one wins unless it is empty.

The value follows these rules:

* Each top-level key is an `http` or `https` registry URL, with no
  credentials, query string, or fragment.
* Under a URL, `"@"` holds the token for the whole registry. `"@org"` holds the
  token for packages in the `@org` scope.
* `authToken` is the only credential field. It is a bearer token.
* `authToken` can reference an environment variable as `${NAME}`.

Each entry also routes packages to its URL. `"@"` makes the URL the default
registry, and `"@org"` sends `@org/*` packages there. This routing has the
following priority, highest first:

1. `--registry` and `--@scope:registry` on the command line
2. `pnpm_config__auth` or `PNPM_CONFIG__AUTH`
3. `_auth` in the global `config.yaml`
4. `pnpm-workspace.yaml`

## Environment variables in auth settings

User-level auth files, `<pnpm config>/auth.ini` and `~/.npmrc`, can reference
environment variables as `${NAME}`:

```ini
//registry.npmjs.org/:_authToken=${NPM_TOKEN}
```

npm's `${NAME?}` form works too. It expands to an empty string
when `NAME` is unset, without a warning. If an empty variable removes an auth
token, pnpm prints a warning that names the key.

pnpm does not expand environment variables in the project `.npmrc` for:

* registry and proxy URLs: `registry`, `@scope:registry`, `proxy`,
  `https-proxy`, `http-proxy`
* keys that start with `//`
* credentials: `_authToken`, `_auth`, `_password`, `username`, `tokenHelper`,
  `cert`, `key`

pnpm ignores such a setting and prints a warning. The project `.npmrc` comes
with the repository, so a malicious repository could otherwise send your CI
tokens to a registry it controls
([GHSA-3qhv-2rgh-x77r](https://github.com/pnpm/pnpm/security/advisories/GHSA-3qhv-2rgh-x77r)).

### Moving a token out of the project .npmrc

If your repository commits a line such as
`//registry.npmjs.org/:_authToken=${NPM_TOKEN}`, use one of these instead.

Pass the token in an environment variable named after the key:

```sh
env "pnpm_config_//registry.npmjs.org/:_authToken=$NPM_TOKEN" pnpm install
```

Shells cannot `export` this name, because it contains `/`, `:`, and `.`. Use
`env` as above, or set the variable in your CI provider's settings. It
overrides the project `.npmrc`, and a command-line option overrides it.

Or use [`_auth`](#_auth), as in the [quick start](#quick-start).

Or save the token in `<pnpm config>/auth.ini` in a CI step:

```sh
pnpm config set //registry.npmjs.org/:_authToken "$NPM_TOKEN"
```

Or move the `${NPM_TOKEN}` line to `~/.npmrc`, or to the file that
[`npmrcAuthFile`](./settings/other.md#npmrcauthfile) names.

In GitHub Actions, `actions/setup-node` with the `registry-url` input already
works. It writes a user-level `.npmrc` and points `NPM_CONFIG_USERCONFIG` at
it. pnpm reads that file, so `NODE_AUTH_TOKEN` keeps working.

If you cannot change every CI pipeline, mark the project `.npmrc` as trusted
with one variable in the CI environment:

```text
PNPM_CONFIG_NPMRC_AUTH_FILE=.npmrc
```

This sets [`npmrcAuthFile`](./settings/other.md#npmrcauthfile). pnpm then reads
the project `.npmrc` as the user-level auth file and expands its variables. A
relative path is resolved against the working directory. A repository cannot
set this variable, because it comes from the environment. pnpm also reads
npm's `NPM_CONFIG_USERCONFIG` if this variable is unset.

:::danger

Use this only where every repository you build is trusted. It turns off this
protection for the checked-out repository, including the rule that only
user-level files can set `tokenHelper`.

:::

The same applies to a registry URL built from an environment variable. Move it
to `~/.npmrc` or set it with `pnpm config set "<key>" <value>`. A URL that is
not secret can be written out in the project `.npmrc`. For registries in
`pnpm-workspace.yaml`, see
[`registries`](./settings/dependency-resolution.md#registries).

## Certificate settings

Like the token settings, these go in `.npmrc` or `auth.ini`.

### ca

* Default: **null** (the CA certificates bundled with Node.js)
* Type: **String, Array or null**

The certificate authority (CA) certificate to trust for TLS connections to the
registry, in PEM format:

```ini
ca="-----BEGIN CERTIFICATE-----\nXXXX\nXXXX\n-----END CERTIFICATE-----"
```

To trust several CAs, use an array:

```ini
ca[]="..."
ca[]="..."
```

Set it to `null` to trust only the well-known CAs.

See also [`strictSsl`](./settings/network.md#strictssl).

### cafile

* Default: **null**
* Type: **path**

The path to a file with one or more CA certificates. Use it instead of `ca` to
keep the certificates in a file.

### &lt;URL&gt;&#58;ca

A CA certificate in PEM format, trusted only for the given registry:

```ini
//registry.example.com/:ca=-----BEGIN CERTIFICATE-----...-----END CERTIFICATE-----
```

### &lt;URL&gt;&#58;cafile

The path to a CA certificate file for the given registry:

```ini
//registry.npmjs.org/:cafile=ca-cert.pem
```

### cert

* Default: **null**
* Type: **String**

The client certificate to send to the registry, in PEM format. The value is
the certificate itself, not a path:

```ini
cert="-----BEGIN CERTIFICATE-----\nXXXX\nXXXX\n-----END CERTIFICATE-----"
```

### &lt;URL&gt;&#58;cert

The client certificate to send to the given registry:

```ini
//registry.example.com/:cert=-----BEGIN CERTIFICATE-----...-----END CERTIFICATE-----
```

### &lt;URL&gt;&#58;certfile

The path to a client certificate file for the given registry:

```ini
//registry.npmjs.org/:certfile=server-cert.pem
```

### key

* Default: **null**
* Type: **String**

The client key to send to the registry, in PEM format. The value is the key
itself, not a path. To read the key from a file, use `<URL>:keyfile`.

```ini
key="-----BEGIN PRIVATE KEY-----\nXXXX\nXXXX\n-----END PRIVATE KEY-----"
```

### &lt;URL&gt;&#58;key

The client key for the given registry:

```ini
//registry.example.com/:key=-----BEGIN PRIVATE KEY-----...-----END PRIVATE KEY-----
```

### &lt;URL&gt;&#58;keyfile

The path to a client key file for the given registry:

```ini
//registry.npmjs.org/:keyfile=server-key.pem
```
