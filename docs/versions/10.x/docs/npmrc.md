---
id: npmrc
title: "Auth & Registry Settings (.npmrc)"
---

The settings on this page must be configured via `.npmrc` files. pnpm uses the npm CLI under the hood for publishing, so these settings need to be in a format that npm can read.

For details on how `.npmrc` files are loaded, see the [`pnpm config`](./cli/config.md) command.

For settings that can be configured in `pnpm-workspace.yaml`, see [Settings (pnpm-workspace.yaml)](./settings.md).

## Environment variables

Values in `.npmrc` files may reference environment variables using the `${NAME}` syntax.

Since v10.34.2, environment variables are **not** expanded in the project and workspace `.npmrc` files (those checked out together with the repository) for the following settings:

* registry and proxy URLs (`registry`, `@scope:registry`, `proxy`, `https-proxy`, `http-proxy`);
* URL-scoped keys (keys starting with `//`);
* credential values (`_authToken`, `_auth`, `_password`, `username`, `tokenHelper`, `cert`, `key`).

A setting that contains a `${...}` placeholder in any of these positions is ignored, and pnpm prints a warning. Repository-controlled `.npmrc` files must not be able to expand environment variables into the URLs pnpm sends requests to, or into the credentials attached to those requests — otherwise a malicious repository could exfiltrate secrets from your environment (such as CI tokens) to an attacker-controlled registry during installation ([GHSA-3qhv-2rgh-x77r](https://github.com/pnpm/pnpm/security/advisories/GHSA-3qhv-2rgh-x77r)).

If your project relied on a committed `.npmrc` containing a line like `//registry.npmjs.org/:_authToken=${NPM_TOKEN}`, move the token to a trusted location instead:

* Write the token to the global config file before installing (for example, in a CI step):

  ```sh
  pnpm config set //registry.npmjs.org/:_authToken "$NPM_TOKEN"
  ```

  `pnpm config set` writes to the global config file by default, not to the project `.npmrc`, so the token never ends up in the repository.

* Or keep the `${NPM_TOKEN}` placeholder line, but put it in the user-level `~/.npmrc` instead of the repository — environment variables are still expanded there.
* Or set the value via an environment variable directly, without any `.npmrc` entry:

  ```sh
  npm_config_//registry.npmjs.org/:_authToken=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
  ```

* In GitHub Actions, `actions/setup-node` with the `registry-url` input writes the auth setting to a user-level `.npmrc` (referenced by the `NPM_CONFIG_USERCONFIG` environment variable, which pnpm honors), so authentication via the `NODE_AUTH_TOKEN` environment variable continues to work.
* If you cannot easily modify each CI pipeline, you may declare the project `.npmrc` trusted by setting a single environment variable in the CI environment (for example, at the organization or workspace level):

  ```text
  NPM_CONFIG_USERCONFIG=.npmrc
  ```

  This makes pnpm read the project's `.npmrc` as the user-level config file (a relative path is resolved against the working directory), so environment variables in it are expanded as before. Because the trust declaration comes from the environment — not from the repository — a malicious repository cannot set it for you.

  :::danger

  Only use this in environments that exclusively build trusted repositories. It disables this protection entirely for the checked-out repository, including the restriction that `tokenHelper` may only be set in user-level config.

  :::

The same rule applies to **registry and proxy URLs** in a project or workspace `.npmrc` (`registry`, `@scope:registry`, `proxy`, `https-proxy`, `http-proxy`). If you used an environment variable to build a registry URL, move the setting to a trusted source — your user-level `~/.npmrc`, the global config (`pnpm config set "<key>" <value>`), a CLI option, or an environment variable. If the URL is not secret, you can also write the resolved value directly in the project `.npmrc`, since only `${...}` placeholders are ignored.

## Registry Settings

### registry

* Default: **https://registry.npmjs.org/**
* Type: **url**

The base URL of the npm package registry (trailing slash included).

### @jsr\:registry

Added in: v10.9.0

* Default: **https://npm.jsr.io/**
* Type: **url**

The base URL of the [JSR] package registry.

[JSR]: cli/add.md#install-from-the-jsr-registry

### &lt;scope&gt;&#58;registry

The npm registry that should be used for packages of the specified scope. For
example, setting `@babel:registry=https://example.com/packages/npm/`
will enforce that when you use `pnpm add @babel/core`, or any `@babel` scoped
package, the package will be fetched from `https://example.com/packages/npm`
instead of the default registry.

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

Since v10.34.2, environment variables are only expanded in trusted locations (the user `~/.npmrc`, the global config, CLI options, and `npm_config_*` environment variables), not in the project or workspace `.npmrc`. See [Environment variables](#environment-variables).

Or you may just use an environment variable directly, without changing `.npmrc` at all:

```
npm_config_//registry.npmjs.org/:_authToken=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

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

## Proxy Settings

### https-proxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing HTTPS requests. If the `HTTPS_PROXY`, `https_proxy`,
`HTTP_PROXY` or `http_proxy` environment variables are set, their values will be
used instead.

If your proxy URL contains a username and password, make sure to URL-encode them.
For instance:

```ini
https-proxy=https://use%21r:pas%2As@my.proxy:1234/foo
```

Do not encode the colon (`:`) between the username and password.

### http-proxy
### proxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing http requests. If the HTTP_PROXY or http_proxy
environment variables are set, proxy settings will be honored by the underlying
request library.

### local-address

* Default: **undefined**
* Type: **IP Address**

The IP address of the local interface to use when making connections to the npm
registry.

### maxsockets

* Default: **networkConcurrency x 3**
* Type: **Number**

The maximum number of connections to use per origin (protocol/host/port combination).

### noproxy

* Default: **null**
* Type: **String**

A comma-separated string of domain extensions that a proxy should not be used for.

## SSL Settings

### strict-ssl

* Default: **true**
* Type: **Boolean**

Whether or not to do SSL key validation when making requests to the registry via
HTTPS.

See also the `ca` option.

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

See also the `strict-ssl` config.

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
