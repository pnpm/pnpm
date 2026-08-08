# get-pnpm

> Installs pnpm as a standalone executable

Installs pnpm the same way [the standalone install script](https://github.com/pnpm/get.pnpm.io) does — a self-contained executable in `PNPM_HOME`, on your `PATH`, with no dependency on Node.js afterwards — for people who would rather not pipe a script into a shell.

## Usage

```sh
npx get-pnpm
```

Install a specific version, major, or dist-tag:

```sh
npx get-pnpm 12         # the latest release of pnpm 12
npx get-pnpm next-12    # the pnpm 12 prerelease lane
npx get-pnpm 11.20.0    # an exact version
```

Then open a new terminal, or source the file the installer names in its output.

## How it works

1. Resolves the requested version against the `@pnpm/exe` dist-tags.
2. Downloads the executable for your platform from the npm registry, verifying the checksum the registry published for it.
3. Runs `pnpm setup`, which installs the executable globally and adds `PNPM_HOME` to your `PATH`.

The download goes to the registry npm is configured with (`npm_config_registry`), not to GitHub. Registries that require authentication are not supported yet.

## Environment variables

| Variable | Description |
| --- | --- |
| `PNPM_VERSION` | Version to install when no argument is given. |
| `PNPM_HOME` | Directory to install pnpm into. |
| `npm_config_registry` | Registry to download pnpm from. |

## Other ways to install pnpm

See [pnpm.io/installation](https://pnpm.io/installation).
