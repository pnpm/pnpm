---
id: npmrc
title: ".npmrc"
---

pnpm gets its configuration from the command line, environment variables, and
`.npmrc` files.

The `pnpm config` command can be used to update and edit the contents of the
user and global `.npmrc` files.

The four relevant files are:

* per-project configuration file (`/path/to/my/project/.npmrc`)
* per-workspace configuration file (the directory that contains the
`pnpm-workspace.yaml` file)
* per-user configuration file (`~/.npmrc`)
* global configuration file (`/etc/npmrc`)

All `.npmrc` files are an [INI-formatted] list of `key = value` parameters.

[INI-formatted]: https://en.wikipedia.org/wiki/INI_file

## Dependency Hoisting Settings

### hoist

Added in: v4.0.0

* Default: **true**
* Type: **boolean**

When `true`, all dependencies are hoisted to `node_modules/.pnpm`. This makes
unlisted dependencies accessible to all packages inside `node_modules`.

### hoist-pattern

Added in: v4.0.0

* Default: **['\*']**
* Type: **string[]**

Tells pnpm which packages should be hoisted to `node_modules/.pnpm`. By
default, all packages are hoisted - however, if you know that only some flawed
packages have phantom dependencies, you can use this option to exclusively hoist
the phantom dependencies (recommended).

For instance:

```sh
hoist-pattern[]=*eslint*
hoist-pattern[]=*babel*
```

### public-hoist-pattern

Added in: v5.2.0

* Default: **['\*types\*', '\*eslint\*', '@prettier/plugin-\*', '\*prettier-plugin-\*']**
* Type: **string[]**

Unlike `hoist-pattern`, which hoists dependencies to a hidden modules directory
inside the virtual store, `public-hoist-pattern` hoists dependencies matching
the pattern to the root modules directory. Hoisting to the root modules
directory means that application code will have access to phantom dependencies,
even if they modify the resolution strategy improperly.

This setting is useful when dealing with some flawed pluggable tools that don't
resolve dependencies properly.

For instance:

```sh
public-hoist-pattern[]=*plugin*
```

Note: Setting `shamefully-hoist` to `true` is the same as setting
`public-hoist-pattern` to `*`.

### shamefully-hoist

Added in: v1.34.0 as `shamefully-flatten`, renamed in v4.0.0

* Default: **false**
* Type: **Boolean**

By default, pnpm creates a semistrict `node_modules`, meaning dependencies have
access to undeclared dependencies but modules outside of `node_modules` do not.
With this layout, most of the packages in the ecosystem work with no issues.
However, if some tooling only works when the hoisted dependencies are in the
root of `node_modules`, you can set this to `true` to hoist them for you.

## Node-Modules Settings

### store-dir

Added in: v4.2.0 as `store`

* Default: **~/.pnpm-store**
* Type: **path**

The location where all the packages are saved on the disk.

The store should be always on the same disk on which installation is happening,
so there will be one store per disk. If there is a home directory on the current
disk, then the store is created in `<home dir>/.pnpm-store`. If there is no
home on the disk, then the store is created at the root of the filesystem. For
example, if installation is happening on a filesystem mounted at `/mnt`,
then the store will be created at `/mnt/.pnpm-store`. The same goes for Windows
systems.

It is possible to set a store from a different disk but in that case pnpm will
copy packages from the store instead of hard-linking them, as hard links are
only possible on the same filesystem.

### modules-dir

Added in: v4.14.0

* Default: **node_modules**
* Type: **path**

The directory in which dependencies will be installed (instead of
`node_modules`).

### node-linker

Added in: v5.9.0

* Default: **undefined**
* Type: **undefined**, **pnp**

Defines what linker should be used for installing Node packages.
By default, pnpm creates a linked modules directory, but the Plug'n'Play
build and resolution strategy is supported as well. Plug'n'Play is an innovative
strategy for Node that is [used by Yarn][pnp].

It is recommended to also set `symlink` setting to `false` when using `pnp` as
your linker.

[pnp]: https://yarnpkg.com/features/pnp

### symlink

Added in: v5.9.0

* Default: **true**
* Type: **Boolean**

When `symlink` is set to `false`, pnpm creates a virtual store directory without
any symlinks. It is a useful setting together with `node-linker=pnp`.

### enable-modules-dir

Added in: v5.15.0

* Default: **false**
* Type: **Boolean**

When `false`, pnpm will not write any files to the modules directory
(`node_modules`). This is useful for when the modules directory is mounted with
filesystem in userspace (FUSE). There is an experimental CLI that allows you to
mount a modules directory with FUSE: [@pnpm/mount-modules].

[@pnpm/mount-modules]: https://www.npmjs.com/package/@pnpm/mount-modules

### virtual-store-dir

Added in: v4.1.0

* Default: **node_modules/.pnpm**
* Types: **path**

The directory with links to the store. All direct and indirect dependencies of
the project are linked into this directory.

This is a useful setting that can solve issues with long paths on Windows. If
you have some dependencies with very long paths, you can select a virtual store
in the root of your drive (for instance `C:\my-project-store`).

Or you can set the virtual store to `.pnpm` and add it to `.gitignore`. This
will make stacktraces cleaner as paths to dependencies will be one directory
higher.

**NOTE:** the virtual store cannot be shared between several projects. Every
project should have its own virtual store (except for in workspaces where the
root is shared).

### package-import-method

Added in: v1.25.0

* Default: **auto**
* Type: **auto**, **hardlink**, **copy**, **clone**

Controls the way packages are imported from the store.

* **auto** - try to clone packages from the store. If cloning is not supported
then hardlink packages from the store. If neither cloning nor linking is
possible, fall back to copying
* **hardlink** - hard link packages from the store
* **copy** - copy packages from the store
* **clone** - clone (AKA copy-on-write or reference link) packages from the store

## Lockfile Settings

### lockfile

Added in: v1.32.0 as `shrinkwrap`

* Default: **true**
* Type: **Boolean**

When set to `false`, pnpm won't read or generate a `pnpm-lock.yaml` file.

### prefer-frozen-lockfile

Added in: v1.37.1 as `prefer-frozen-shrinkwrap`

* Default: **true** (from v1.38.0)
* Type: **Boolean**

When set to `true` and the available `pnpm-lock.yaml` satisfies the
`package.json` dependencies directive, a headless installation is performed. A
headless installation skips all dependency resolution as it does not need to
modify the lockfile.


## Registry & Authentication Settings

### registry

* Default: **https://registry.npmjs.org/**
* Type: **url**

The base URL of the npm package registry (trailing slash included).

#### &lt;scope\>:registry

The npm registry that should be used for packages of the specified scope. For
example, setting `@babel:registry=https://example.com/packages/npm/`
will enforce that when you use `pnpm add @babel/core`, or any `@babel` scoped
package, the package will be fetched from `https://example.com/packages/npm`
instead of the default registry.

### &lt;URL\>:_authToken

Define the authentication bearer token to use when accessing the specified
registry. For example:

```sh
//registry.npmjs.org/:_authToken=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

You may also use an environment variable. For example:

```
//registry.npmjs.org/:_authToken=${NPM_TOKEN}
```

### &lt;URL\>:always-auth

* Default: **false**
* Type: **Boolean**

Force pnpm to always require authentication (even for GET requests) when
accessing the specified registry. For example:

```sh
@babel:registry=https://gitlab.com/api/v4/packages/npm/
//gitlab.com/api/v4/packages/npm/:always-auth=true

registry=https://registry.npmjs.org/
//registry.npmjs.org/:always-auth=true
```

## Request Settings

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

### cert

* Default: **null**
* Type: **String**

A client certificate to pass when accessing the registry. Values should be in
PEM format (AKA "Base-64 encoded X.509 (.CER)"). For example:

```test
cert="-----BEGIN CERTIFICATE-----\nXXXX\nXXXX\n-----END CERTIFICATE-----"
```

It is not the path to a certificate file (and there is no `certfile` option).

### https-proxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing HTTPS requests. If the `HTTPS_PROXY`, `https_proxy`,
`HTTP_PROXY` or `http_proxy` environment variables are set, their values will be
used instead.

### key

* Default: **null**
* Type: **String**

A client key to pass when accessing the registry. Values should be in PEM format
(AKA "Base-64 encoded X.509 (.CER)"). For example:

```sh
key="-----BEGIN PRIVATE KEY-----\nXXXX\nXXXX\n-----END PRIVATE KEY-----"
```

It is not the path to a key file (and there is no `keyfile` option).

### local-address

* Default: **undefined**
* Type: **IP Address**

The IP address of the local interface to use when making connections to the npm
registry. Must be IPv4 in versions of Node prior to 12.x.

### proxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing http requests. If the HTTP_PROXY or http_proxy
environment variables are set, proxy settings will be honored by the underlying
request library.

### noproxy

Added in: v5.18.8

* Default: **null**
* Type: **String**

A comma-separated string of domain extensions that a proxy should not be used for.

### strict-ssl

* Default: **true**
* Type: **Boolean**

Whether or not to do SSL key validation when making requests to the registry via
HTTPS.

See also the `ca` option.

### network-concurrency

* Default: **16**
* Type: **Number**

Controls the maximum number of HTTP(S) requests to process simultaneously.

### fetch-retries

* Default: **2**
* Type: **Number**

How many times to retry if pnpm fails to fetch from the registry.

### fetch-retry-factor

* Default: **10**
* Type: **Number**

The exponential factor for retry backoff.

### fetch-retry-mintimeout

* Default: **10000 (10 seconds)**
* Type: **Number**

The minimum (base) timeout for retrying requests.

### fetch-retry-maxtimeout

* Default: **60000 (1 minute)**
* Type: **Number**

The maximum fallback timeout to ensure the retry factor does not make requests
too long.

## CLI Settings

### [no-]color

Added in: v4.1.0

* Default: **auto**
* Type: **auto**, **always**, **never**

Controls colors in the output.

* **auto** - output uses colors when the standard output is a terminal or TTY.
* **always** - ignore the difference between terminals and pipes. You’ll rarely
  want this; in most scenarios, if you want color codes in your redirected
  output, you can instead pass a `--color` flag to the pnpm command to force it
  to use color codes. The default setting is almost always what you’ll want.
* **never** - turns off colors. This is the setting used by `--no-color`.

### loglevel

Added in: v4.13.0

* Default: **info**
* Type: **debug**, **info**, **warn**, **error**

Any logs at or higher than the given level will be shown.
You can instead pass `--silent` to turn off all output logs.

### strict-peer-dependencies

Added in: v2.15.0

* Default: **false**
* Type: **Boolean**

If this is enabled, commands will fail if there is a missing or invalid peer
dependency in the tree.

### use-beta-cli

Added in: v3.6.0

* Default: **false**
* Type: **Boolean**

Experimental option that enables beta features of the CLI. This means that you
may get some changes to the CLI functionality that are breaking changes, or
potentially bugs.

### recursive-install

Added in: v5.4.0

* Default: **true**
* Type: **Boolean**

If this is enabled, the primary behaviour of `pnpm install` becomes that of
`pnpm install -r`, meaning the install is performed on all workspace or
subdirectory packages.

Else, `pnpm install` will exclusively build the package in the current
directory.

### engine-strict

* Default: **false**
* Type: **Boolean**

If this is enabled, pnpm will not install any package that claims to not be
compatible with the current Node version.

Regardless of this configuration, installation will always fail if a project
(not a dependency) specifies an incompatible version in its `engines` field.

### npm-path

Added in: v4.8.0

* Type: **path**

The location of the npm binary that pnpm uses for some actions, like publishing.

## Build Settings

### child-concurrency

* Default: **5**
* Type: **Number**

The maximum number of child processes to allocate simultaneously to build
node_modules.

### side-effects-cache

Added in: v1.31.0

* Default: **false**
* Type: **Boolean**
* Stability: **Experimental**

Use and cache the results of (pre/post)install hooks.

### side-effects-cache-readonly

Added in: v1.31.0

* Default: **false**
* Type: **Boolean**
* Stability: **Experimental**

Only use the side effects cache if present, do not create it for new packages.

### unsafe-perm

* Default: **false** IF running as root, ELSE **true**
* Type: **Boolean**

Set to true to enable UID/GID switching when running package scripts.
If set explicitly to false, then installing as a non-root user will fail.

## Other Settings

### use-running-store-server

Added in: v2.5.0

* Default: **false**
* Type: **Boolean**

Only allows installation with a store server. If no store server is running,
installation will fail.

### save-prefix

* Default: **'^'**
* Type: **String**

Configure how versions of packages installed to a `package.json` file get
prefixed.

For example, if a package has version `1.2.3`, by default its version is set to
`^1.2.3` which allows minor upgrades for that package, but after
`pnpm config set save-prefix='~'` it would be set to `~1.2.3` which only allows
patch upgrades.

This setting is ignored when the added package has a range specified. For
instance, `pnpm add foo@2` will set the version of `foo` in `package.json` to
`2`, regardless of the value of `save-prefix`.

### tag

* Default: **latest**
* Type: **String**

If you `pnpm add` a package and you don't provide a specific version, then it
will install the package at the version registered under the tag from this
setting.

This also sets the tag that is added to the `package@version` specified by the
`pnpm tag` command if no explicit tag is given.

### global-dir

Added in: v4.2.0

* Default: **&lt;path to node\>/pnpm-global**
* Type: **path**

Specify a custom directory to store global packages.
