---
id: installation
title: Installation
---

## Prerequisites

If you don't use the standalone script or `@pnpm/exe` to install pnpm, then you need to have Node.js (at least v22) to be installed on your system.

## Using a standalone script

You may install pnpm even if you don't have Node.js installed, using the following scripts.

### On Windows

:::warning

Sometimes, Windows Defender may block our executable if you install pnpm this way.

Due to this issue, we currently recommend installing pnpm using [npm](#using-npm) on Windows.

:::

Using PowerShell:

```powershell
$env:PNPM_VERSION="latest-11"; Invoke-WebRequest https://get.pnpm.io/install.ps1 -UseBasicParsing | Invoke-Expression
```

On Windows, Microsoft Defender can significantly slow down installation of packages. You can add pnpm to Microsoft Defender's list
of excluded folders in a PowerShell window with administrator rights by executing:

```powershell
Add-MpPreference -ExclusionPath $(pnpm store path)
```

### On POSIX systems

:::warning Not supported on Intel macOS

The standalone script does not run on Intel Macs (`darwin-x64`). Install pnpm through [npm](#using-npm) instead. See [#11423](https://github.com/pnpm/pnpm/issues/11423) for context.

:::

```sh
curl -fsSL https://get.pnpm.io/install.sh | env PNPM_VERSION=latest-11 sh -
```

If you don't have curl installed, you would like to use wget:

```sh
wget -qO- https://get.pnpm.io/install.sh | env PNPM_VERSION=latest-11 sh -
```

:::info Linux runtime requirements

The install script picks a glibc or musl build based on your system's libc, and a separate musl build is provided for Alpine and other musl-based distros. The glibc build requires glibc 2.27 or newer plus `libatomic.so.1` — both are present on most full distros but may be missing from minimal container images. If you see `error while loading shared libraries: libatomic.so.1`, install it with your distro's package manager:

- Debian/Ubuntu: `apt-get install -y libatomic1`
- Fedora/RHEL: `dnf install -y libatomic`

:::

:::tip

You may use the [pnpm runtime] command then to install Node.js.

:::

### In a Docker container

```sh
# bash
wget -qO- https://get.pnpm.io/install.sh | env PNPM_VERSION=latest-11 ENV="$HOME/.bashrc" SHELL="$(which bash)" bash -
# sh
wget -qO- https://get.pnpm.io/install.sh | env PNPM_VERSION=latest-11 ENV="$HOME/.shrc" SHELL="$(which sh)" sh -
# dash
wget -qO- https://get.pnpm.io/install.sh | env PNPM_VERSION=latest-11 ENV="$HOME/.dashrc" SHELL="$(which dash)" dash -
```

### Installing a specific version

Prior to running the install script, you may optionally set an env variable `PNPM_VERSION` to install a specific version of pnpm:

```sh
curl -fsSL https://get.pnpm.io/install.sh | env PNPM_VERSION=<version> sh -
```

## Using npm

```sh
npx get-pnpm latest-11
```

:::tip

Do you wanna use pnpm on CI servers? See: [Continuous Integration](./continuous-integration.md).

:::

## Compatibility

Here is a list of past pnpm versions with respective Node.js version support.

| Node.js    | pnpm 8 | pnpm 9 | pnpm 10 | pnpm 11 |
|------------|--------|--------|---------|---------|
| Node.js 14 | ❌     | ❌     | ❌      | ❌      |
| Node.js 16 | ✔️      | ❌     | ❌      | ❌      |
| Node.js 18 | ✔️      | ✔️      | ✔️       | ❌      |
| Node.js 20 | ✔️      | ✔️      | ✔️       | ❌      |
| Node.js 22 | ✔️      | ✔️      | ✔️       | ✔️       |
| Node.js 24 | ✔️      | ✔️      | ✔️       | ✔️       |
| Node.js 26 | ✔️      | ✔️      | ✔️       | ✔️       |

## Troubleshooting

If pnpm is broken and you cannot fix it by reinstalling, you might need to remove it manually from the PATH.

Let's assume you have the following error when running `pnpm install`:

```
C:\src>pnpm install
internal/modules/cjs/loader.js:883
  throw err;
  ^



Error: Cannot find module 'C:\Users\Bence\AppData\Roaming\npm\pnpm-global\4\node_modules\pnpm\bin\pnpm.js'
←[90m    at Function.Module._resolveFilename (internal/modules/cjs/loader.js:880:15)←[39m
←[90m    at Function.Module._load (internal/modules/cjs/loader.js:725:27)←[39m
←[90m    at Function.executeUserEntryPoint [as runMain] (internal/modules/run_main.js:72:12)←[39m
←[90m    at internal/main/run_main_module.js:17:47←[39m {
  code: ←[32m'MODULE_NOT_FOUND'←[39m,
  requireStack: []
}
```

First, try to find the location of pnpm by running: `which pnpm`. If you're on Windows, run `where.exe pnpm.*`.
You'll get the location of the pnpm command, for instance:

```
$ which pnpm
/c/Program Files/nodejs/pnpm
```

Now that you know where the pnpm CLI is, open that directory and remove any pnpm-related files (`pnpm.cmd`, `pnpx.cmd`, `pnpm`, etc).
Once done, install pnpm again and it should work as expected.

## Updating pnpm

To update to the newest pnpm 11, run the [`self-update`] command with the major:

```
pnpm self-update 11
```

A bare `pnpm self-update` follows npm's `latest` tag, which now points at pnpm
12. See [Installing pnpm v12](./cli/self-update.md#installing-pnpm-v12-the-rust-port) if that
is what you want.

[`self-update`]: ./cli/self-update.md

## Uninstalling pnpm

If you need to remove the pnpm CLI from your system and any files it has written to your disk, see [Uninstalling pnpm].

[Uninstalling pnpm]: ./uninstall.md
[pnpm runtime]: ./cli/runtime.md
