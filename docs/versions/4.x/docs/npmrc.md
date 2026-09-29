---
id: npmrc
title: ".npmrc"
original_id: npmrc
---

pnpm gets its config settings from the command line, environment variables, and `.npmrc` files.

The `pnpm config` command can be used to update and edit the contents of the user and global `.npmrc` files.

The four relevant files are:

* per-project config file (/path/to/my/project/.npmrc)
* per-workspace config file (the directory that contains the `pnpm-workspace.yaml` file)
* per-user config file (`~/.npmrc`)
* global config file (`$PREFIX/etc/npmrc`)

All `.npmrc` files are an ini-formatted list of `key = value` parameters.

## Settings

### always-auth

* Default: **false**
* Type: **Boolean**

Force pnpm to always require authentication when accessing the registry, even for GET requests.

### ca

* Default: **The npm CA certificate**
* Type: **String, Array or null**

The Certificate Authority signing certificate that is trusted for SSL connections to the registry.
Values should be in PEM format (Windows calls it "Base-64 encoded X.509 (.CER)")
with newlines replaced by the string "\n". For example:

```text
ca="-----BEGIN CERTIFICATE-----\nXXXX\nXXXX\n-----END CERTIFICATE-----"
```

Set to null to only allow "known" registrars, or to a specific CA cert to trust only that specific signing authority.

Multiple CAs can be trusted by specifying an array of certificates:

```test
ca[]="..."
ca[]="..."
```

See also the `strict-ssl` config.

### cafile

* Default: **null**
* Type: **path**

A path to a file containing one or multiple Certificate Authority signing certificates.
Similar to the ca setting, but allows for multiple CA’s, as well as for the
CA information to be stored in a file on disk.

### cert

* Default: **null**
* Type: **String**

A client certificate to pass when accessing the registry. Values should be in PEM format
(Windows calls it "Base-64 encoded X.509 (.CER)") with newlines replaced by the string "\n". For example:

```test
cert="-----BEGIN CERTIFICATE-----\nXXXX\nXXXX\n-----END CERTIFICATE-----"
```

It is not the path to a certificate file (and there is no "certfile" option).

### color

Added in: v4.1.0

* Default: **auto**
* Type: **auto**, **always**, **never**

Controls colors in the output.

* **auto** - output uses colors when the stdout is a TTY (i.e. when the output goes straight to a terminal).
* **always** - ignore the difference between terminals and pipes. You’ll rarely want this; in most scenarios,
  if you want color codes in your redirected output, you can instead pass a `--color` flag to the pnpm command
  to force it to use color codes. The default setting is almost always what you’ll want.
* **never** - turns off colors. You can also turn off colors via the `--no-color` flag.

### store-dir

Added in: v4.2.0 (renamed from `store`)

* Default: **~/.pnpm-store**
* Type: **path**

The location where all the packages are saved on the disk.

The store should be always on the same disk on which installation is happening. So there will be one store per disk.
If there is a home directory on the current disk, then the store is created in `<home dir>/.pnpm-store`. If there is no
homedir on the disk, then the store is created in the root. For example, if installation is happening on disk `D`
then the store will be created in `D:\.pnpm-store`.

It is possible to set a store from a different disk but in that case pnpm will copy, not link, packages from the store.
Hard links are possible only inside a filesystem.

### modules-dir

Added in: v4.14.0

* Default: **node_modules**
* Type: **path**

The directory in which dependencies will be installed (instead of `node_modules`).

### network-concurrency

* Default: **16**
* Type: **Number**

Controls the maximum number of HTTP requests that can be done simultaneously.

### child-concurrency

* Default: **5**
* Type: **Number**

Controls the number of child processes run parallelly to build node modules.

### lock

* Default: **true**
* Type: **Boolean**

Dangerous! If false, the store is not locked. It means that several installations using the same
store can run simultaneously.

Can be passed in via a CLI option. `--no-lock` to set it to false. E.g.: `pnpm install --no-lock`.

> If you experience issues similar to the ones described in [#594](https://github.com/pnpm/pnpm/issues/594), use this option to disable locking.
> In the meanwhile, we'll try to find a solution that will make locking work for everyone.

### loglevel

Added in: v4.13.0

* Default: **info**
* Type: **debug**, **info**, **warn**, **error**

What level of logs to report. Any logs at or higher than the given level will be shown.
Or use `--silent` to turn off all logging.

### independent-leaves

* Default: **false**
* Type: **Boolean**

If true, symlinks leaf dependencies directly from the global store. Leaf dependencies are
packages that have no dependencies of their own. Setting this config to `true` might break some packages
that rely on location but gives an average of **8% installation speed improvement**.

### verify-store-integrity

Added in: v1.8.0

* Default: **true**
* Type: **Boolean**

If false, doesn't check whether packages in the store were mutated.

### virtual-store-dir

Added in: v4.1.0

* Default: **node_modules/.pnpm**
* Types: **path**

The directory with links to the store. All direct and indirect dependencies of the project are linked into this directory.

This is a useful setting that can solve issues with long paths on Windows. If you have some dependencies with very long paths,
you can select a virtual store in the root of your drive (for instance `C:\my-project-store`).

Or you can set the virtual store to `.pnpm` and add it to `.gitignore`. This will make the stacktraces nicer as paths to
dependencies will have one directory less.

**NOTE:** the virtual store cannot be shared between several projects. Every project should have its own virtual store.

### package-import-method

Added in: v1.25.0

* Default: **auto**
* Type: **auto**, **hardlink**, **copy**, **clone**

Controls the way packages are imported from the store.

* **auto** - try to clone packages from the store. If cloning is not supported then hardlink packages from the store. If neither cloning nor linking is possible, falls back to copying
* **hardlink** - hardlink packages from the store
* **copy** - copy packages from the store
* **clone** - clone (aka copy-on-write or reflink) packages from the store

### publish-branch

Added in: v4.11.0

* Default: **master**
* Types: **String**

The main branch of the repository which is used for publishing the latest changes.

### lockfile

Added in: v1.32.0 (initially named `shrinkwrap`)

* Default: **true**
* Type: **Boolean**

When set to `false`, pnpm won't read or generate a `pnpm-lock.yaml` file.

### prefer-frozen-lockfile

Added in: v1.37.1 (initially named `prefer-frozen-shrinkwrap`)

* Default: **true** (from v1.38.0)
* Type: **Boolean**

When `true` and the available `pnpm-lock.yaml` satisfies the `package.json`
then a headless installation is performed. A headless installation is faster than a regular one
because it skips dependencies resolution and peers resolution.

### use-running-store-server

Added in: v2.5.0

* Default: **false**
* Type: **Boolean**

Only allows installation with a store server. If no store server is running, installation will fail.

### side-effects-cache

Added in: v1.31.0

> Stability: Experimental

* Default: **false**
* Type: **Boolean**

Use and cache the results of (pre/post)install hooks.

### side-effects-cache-readonly

Added in: v1.31.0

> Stability: Experimental

* Default: **false**
* Type: **Boolean**

Only use the side effects cache if present, do not create it for new packages.

### hoist

Added in: v4.0.0

* Default: **true**
* Type: **boolean**

When `true`, all dependencies are hoisted to `node_modules/.pnpm`. This makes unlisted dependencies accessible to
all packages inside `node_modules`.

### hoist-pattern

Added in: v4.0.0

* Default: **['\*']**
* Type: **string[]**

Tells pnpm, which packages should be hoisted to `node_modules/.pnpm`. By default, all packages are hoisted.
However, if you know that only some buggy packages are requiring unlisted dependencies, you may hoist just them.

For instance:

```
hoist-pattern[]=*eslint*
hoist-pattern[]=*babel*
```

### shamefully-hoist

Added in: v1.34.0 (Renamed from `shamefully-flatten` in v4.0.0)

* Default: **false**
* Type: **Boolean**

By default, pnpm creates a semistrict `node_modules`. It means that your dependencies have access to undeclared dependencies
but your code does not. With this layout, most of the packages in the ecosystem work with no issues.
However, if some tooling only works when the hoisted dependencies are in the root of `node_modules`, you can set this config to `true`.

### strict-peer-dependencies

Added in: v2.15.0

* Default: **false**
* Type: **Boolean**

If true, commands fail on missing or invalid peer dependencies.

### resolution-strategy

Added in: v3.1.0

* Default: **fewer-dependencies**
* Type: **fast**, **fewer-dependencies**

Sets the resolutions strategy used during installation.

* **fewer-dependencies** - the default resolution strategy. Already installed dependencies are preferred even if newer versions satisfy a range
* **fast** - speed is preferred over deduplication

### use-beta-cli

Added in: v3.6.0

* Default: **false**
* Type: **Boolean**

When `true`, beta features of the CLI are used. This means that you may get some changes to the CLI functionality
that are breaking changes.

### engine-strict

* Default: **false**
* Type: **Boolean**

If set to true, then pnpm will stubbornly refuse to install (or even consider installing) any package that claims
to not be compatible with the current Node.js version.

Regardless of this config, installation will always fail when a project (not a dependency) will specify an
incompatible pnpm version in its `engines` field.

### fetch-retries

* Default: **2**
* Type: **Number**

The "retries" config for the retry module to use when fetching packages from the registry.

### fetch-retry-factor

* Default: **10**
* Type: **Number**

The "factor" config for the retry module to use when fetching packages.

### fetch-retry-mintimeout

* Default: **10000 (10 seconds)**
* Type: **Number**

The "minTimeout" config for the retry module to use when fetching packages.

### fetch-retry-maxtimeout

* Default: **60000 (1 minute)**
* Type: **Number**

The "maxTimeout" config for the retry module to use when fetching packages.

### https-proxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing https requests. If the `HTTPS_PROXY` or `https_proxy` or
`HTTP_PROXY` or `http_proxy` environment variables are set, proxy settings will be honored by the underlying request library.

### key

* Default: **null**
* Type: **String**

A client key to pass when accessing the registry. Values should be in PEM format with newlines replaced by the string "\n". For example:

```text
key="-----BEGIN PRIVATE KEY-----\nXXXX\nXXXX\n-----END PRIVATE KEY-----"
```

It is not the path to a key file (and there is no "keyfile" option).

### local-address

* Default: **undefined**
* Type: **IP Address**

The IP address of the local interface to use when making connections to the npm registry. Must be IPv4 in versions of Node prior to 0.12.

### proxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing http requests. If the HTTP_PROXY or http_proxy environment variables are set, proxy settings will be honored by the underlying request library.

### registry

* Default: **https://registry.npmjs.org/**
* Type: **url**

The base URL of the npm package registry.

### save-prefix

* Default: **'^'**
* Type: **String**

Configure how versions of packages installed to a `package.json` file get prefixed.

For example, if a package has version `1.2.3`, by default its version is set to `^1.2.3`
which allows minor upgrades for that package, but after `pnpm config set save-prefix='~'`
it would be set to `~1.2.3` which only allows patch upgrades.

This config is ignored when the added package has a range specified. For instance,
`pnpm add foo@2` will add `2` to `package.json`, regardless of the value of `save-prefix`.

### strict-ssl

* Default: **true**
* Type: **Boolean**

Whether or not to do SSL key validation when making requests to the registry via https.

See also the `ca` config.

### tag

* Default: **latest**
* Type: **String**

If you ask pnpm to install a package and don’t tell it a specific version, then it will install the specified tag.

Also the tag that is added to the `package@version` specified by the `pnpm tag` command, if no explicit tag is given.

### unsafe-perm

* Default: **false if running as root, true otherwise**
* Type: **Boolean**

Set to true to suppress the UID/GID switching when running package scripts.
If set explicitly to false, then installing as a non-root user will fail.

### git-checks

Added in: v4.11.0

* Default : **false**
* Type: **Boolean**

When true, `pnpm publish` checks if current branch is your publish branch
(master by default), clean, and up-to-date.

### global-dir

Added in: v4.2.0

* Default: **&lt;Node path>/pnpm-global**
* Type: **path**

Specify a custom directory to store global packages.

### npm-path

Added in: v4.8.0

* Type: **path**

The location of the npm binary that pnpm uses for some actions (like publishing).
