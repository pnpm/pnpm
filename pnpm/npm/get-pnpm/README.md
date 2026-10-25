# get-pnpm

> Installs pnpm as a standalone executable

Installs pnpm the same way [the standalone install script](https://github.com/pnpm/get.pnpm.io) does — a self-contained executable in `PNPM_HOME`, on your `PATH`, with no dependency on Node.js afterwards — for people who would rather not pipe a script into a shell.

## Usage

```sh
npx get-pnpm
```

This is a permanent install that edits your shell configuration (see
[What it changes](#what-it-changes)). To run pnpm once in someone else's
project, `npx pnpm install` needs neither.

Install a specific version, major, range, or dist-tag:

```sh
npx get-pnpm 12         # the latest release of pnpm 12
npx get-pnpm next-12    # the pnpm 12 prerelease lane
npx get-pnpm 11.20.0    # an exact version
npx get-pnpm "^12.1.0"  # the version npm would install for a range
```

Without a version, and without `PNPM_VERSION`, it installs the version the project
in the current directory pins, so CI and Docker builds do not have to name it a
second time. It reads `devEngines.packageManager`, or else `packageManager`, from
the project's manifest (the workspace root's, for a workspace project) the way
pnpm reads it to switch versions. A range resolves to the version `pnpm-lock.yaml`
records while that is still in range, so the pnpm installed is the one the project
then runs, with no second download. Outside a project, or in one that pins no
pnpm, it installs `latest`.

Then open a new terminal, or source the file the installer names in its output.

If `pnpm --version` still prints an older version, another `pnpm` — from
`npm install -g pnpm`, Homebrew, Corepack, or Volta — comes before `$PNPM_HOME/bin`
on your `PATH`. The installer looks for one when it is done and says which it found
and the command that removes it.

## What it changes

- **`PNPM_HOME`** — pnpm is installed here: `$XDG_DATA_HOME/pnpm` when that is set,
  otherwise `~/.local/share/pnpm` on Linux, `~/Library/pnpm` on macOS, and
  `%LOCALAPPDATA%\pnpm` on Windows.
- **Your shell configuration** — on Linux and macOS, `pnpm setup` adds a block like
  this to the rc file of your shell (`~/.bashrc`, `~/.zshrc`,
  `~/.config/fish/config.fish`, or `~/.config/nushell/env.nu`), and says which file
  it changed:

  ```sh
  # pnpm
  export PNPM_HOME='/home/me/.local/share/pnpm'
  case ":$PATH:" in
    *":$PNPM_HOME/bin:"*) ;;
    *) export PATH="$PNPM_HOME/bin:$PATH" ;;
  esac
  # pnpm end
  ```

  On Windows, it sets a `PNPM_HOME` user environment variable instead and adds
  `%PNPM_HOME%\bin` to your user `Path`.

To leave your shell configuration and the Windows registry alone, pass
`--no-modify-path` (or set `PNPM_NO_MODIFY_PATH=1`). pnpm is still installed in
`PNPM_HOME`, and `pnpm setup` prints the lines to add yourself. This needs a pnpm
version whose `pnpm setup` has that option; with an older one, the installer stops
before changing anything.

```sh
npx get-pnpm --no-modify-path
```

## Uninstalling

1. Delete the `PNPM_HOME` directory, e.g. `rm -rf ~/.local/share/pnpm`.
2. On Linux and macOS, delete the lines from `# pnpm` to `# pnpm end` in the rc file
   the installer named. On Windows, remove the `PNPM_HOME` variable and the
   `%PNPM_HOME%\bin` entry of `Path` under *Edit environment variables for your
   account*.
3. Open a new terminal.

The installer prints these steps when it finishes. See
[pnpm.io/uninstall](https://pnpm.io/uninstall) for removing global packages and the
store first.

## How it works

1. Resolves the requested version against the `pnpm` dist-tags.
2. Downloads the executable for your platform from the npm registry.
3. Checks npm's signature over the package's checksum, then the download against that checksum.
4. Runs `pnpm setup`, which installs the executable globally and adds `PNPM_HOME` to your `PATH`.

Step 3 is what makes step 2 worth trusting. The registry publishes both the tarball
and its checksum, so a checksum taken from it proves nothing on its own; npm also
signs `<name>@<version>:<integrity>` with a key that this package pins, and a
download that fails either check is refused rather than installed.

A registry proxy that strips the signatures from the version document but keeps
them in the package document (JFrog Artifactory does) is handled: the signatures
are read from there instead, and still have to cover the checksum the download
is checked against.

The download goes to the registry npm is configured with (`npm_config_registry`), not to GitHub — including the tarball itself, so a registry that proxies npm and hands back an npmjs.org URL does not send the download off the mirror.

### Private registries

The CLI reads registry-scoped `_authToken` entries from the project, user, and global npm config files. Values may reference environment variables the same way npmrc files do:

```ini
registry=https://registry.example.com/npm/
//registry.example.com/npm/:_authToken=${NPM_TOKEN}
```

An equivalent `npm_config_//registry.example.com/npm/:_authToken` environment entry is also accepted when the environment can represent that name. Credentials embedded in `npm_config_registry` are converted to HTTP Basic authentication and removed from the request URL.

Authentication from npm config is sent only to the configured registry origin and the selected token's path scope. If registry metadata names a tarball on another origin or outside that scope, the credentials are not forwarded there. A token from npm config is sent only over HTTPS or to the local machine. Credentials embedded in `npm_config_registry` are sent to the URL they are embedded in.

A registry behind a private certificate authority is reached with the `ca`, `ca[]`, and `cafile` settings of the same files, top-level or scoped to the registry. They are read as pnpm reads them, and a relative `cafile` is resolved against the directory of the npmrc that names it:

```ini
cafile=certs/company-ca.pem
//registry.example.com/npm/:ca="-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"
```

These CAs are trusted in addition to the ones Node.js trusts. Node.js 22.19 or 24.5 is needed to add them after startup. On older releases, set `NODE_EXTRA_CA_CERTS` to a file holding them.

## Environment variables

| Variable | Description |
| --- | --- |
| `PNPM_VERSION` | Version to install when no argument is given. |
| `PNPM_HOME` | Directory to install pnpm into. |
| `PNPM_NO_MODIFY_PATH` | Set to `1` for the same as `--no-modify-path`. |
| `npm_config_registry` | Registry to download pnpm from. |
| `npm_config_*:_authToken` | Registry-scoped bearer token. Project, user, and global npmrc tokens are also read. |
| `npm_config_ca`, `npm_config_cafile` | CA certificates to trust for the registry. Project, user, and global npmrc CA settings are also read. |
| `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` | Proxy to download through, and hosts to reach directly. Applied on Node 24.14 and later; earlier releases go direct. |

## Using it from a program

A caller that manages its own directory — a CI action, say — wants
`downloadPnpm`, which verifies and places the executable and nothing else. It
does not run `pnpm setup` or edit your `PATH`:

```js
import { downloadPnpm } from 'get-pnpm'

const { version, binPath } = await downloadPnpm({
  versionSpec: 'next-12',
  registry: 'https://registry.npmjs.org/',
  dest: '/opt/pnpm',
  headers: { authorization: 'Bearer …' },
})
```

A caller that already knows the exact version, and already has everything else
pnpm ships with, wants `downloadPnpmExecutable`: it places the executable at a
path you name and touches nothing else. No dist-tags are resolved, so no
packument is fetched, and the `dist/` tree that travels beside the executable is
left to the caller. Corepack is the case it exists for — it unpacks the `pnpm`
package itself but installs none of its dependencies, so the executable has to
arrive separately.

```js
import { downloadPnpmExecutable } from 'get-pnpm'

await downloadPnpmExecutable({
  version: '12.0.0',
  registry: 'https://registry.npmjs.org/',
  destPath: '/opt/pnpm/pnpm',
  headers: { authorization: 'Bearer …' },
})
```

A program that Corepack runs, as the `pnpm` package's launcher is, takes all
of its options from `corepackDownloadOptions`. It reads the registry,
credentials, and CA certificates from `.npmrc` the way pnpm reads them for its
own packages, then applies Corepack's settings: `COREPACK_NPM_REGISTRY` and its
credentials, and `COREPACK_INTEGRITY_KEYS`. It reads the project `.npmrc` only
while the signature is checked, and throws when `COREPACK_ENABLE_NETWORK` is
`0`. The proxy comes from pnpm's config, as pnpm reads it to download itself:
`PNPM_CONFIG_HTTPS_PROXY` and its siblings, the global `config.yaml`, and the
user `.npmrc`, then `HTTPS_PROXY`, `HTTP_PROXY`, and `NO_PROXY`.

```js
import { corepackDownloadOptions, downloadPnpmExecutable } from 'get-pnpm'

await downloadPnpmExecutable({
  version: '12.0.0',
  destPath: '/opt/pnpm/pnpm',
  ...corepackDownloadOptions(),
})
```

It verifies exactly what `downloadPnpm` does, and reads the archive in-process
rather than shelling out to `tar`, so it works where no `tar` is on the PATH.
Placement is atomic and tolerates losing a race with a concurrent call.

Pass `keys` to trust a signing key other than the pinned npm one, or
`verifySignature: false` to download from a registry that carries no npm
signatures at all — a private mirror that re-published the package. The checksum
is still enforced either way; what is lost is the assurance that it came from
somewhere other than the host that served the bytes.

## Development

The package lives in the pnpm/pnpm workspace:

```sh
pnpm install
pnpm --filter=get-pnpm test   # builds, then runs the tests against the build output
pnpm lint:ts                  # from the repository root
```

The command runs on Node.js 22.13 and newer. The test runner needs 22.18 or
newer, which is where Node runs TypeScript without a flag. The download path
also runs on Node.js 18, inside the `pnpm` package, so the build targets ES2022.

## Other ways to install pnpm

See [pnpm.io/installation](https://pnpm.io/installation). The shell and PowerShell
installers in [pnpm/get.pnpm.io](https://github.com/pnpm/get.pnpm.io) do the same
job for machines without Node.js, and pin the same npm signing keys.
