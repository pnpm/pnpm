---
id: installation
title: Installation
---

## Prerequisites

pnpm 12 is a native executable and does not require Node.js after it is installed. Installing it through npm requires Node.js 22.13 or newer.

pnpm 12 is the current release line — the `latest` tag on npm points at it, so the commands below install pnpm 12 without naming a version.

### Supported platforms

pnpm 12 ships a prebuilt binary for each of these targets:

| Operating system | Architectures |
| --- | --- |
| Linux (glibc) | x64, arm64, riscv64, ppc64le, s390x |
| Linux (musl) | x64, arm64 |
| macOS | arm64, x64 |
| Windows | x64, arm64 |
| FreeBSD | x64 |
| Android | arm64, x64 |

FreeBSD, ppc64le, s390x, RISC-V, and Android were added in v12.4.0. On a target with no binary, install the JavaScript [pnpm 11](https://www.npmjs.com/package/pnpm/v/11) instead.

## Using pnpm

If you already have pnpm v11.10.0 or newer, update directly to pnpm 12:

```sh
pnpm self-update
```

Inside a project that pins pnpm through the `packageManager` field, [`self-update`] only updates that pin instead of installing pnpm globally.

## Using a standalone script

You may install pnpm even if you don't have Node.js installed, using the following scripts.

### On Windows

:::warning

Sometimes, Windows Defender may block our executable if you install pnpm this way.

Due to this issue, we currently recommend installing pnpm using [npm](#using-npm) on Windows.

:::

Using PowerShell:

```powershell
Invoke-WebRequest https://get.pnpm.io/install.ps1 -UseBasicParsing | Invoke-Expression
```

On Windows, Microsoft Defender can significantly slow down installation of packages. You can add pnpm to Microsoft Defender's list
of excluded folders in a PowerShell window with administrator rights by executing:

```powershell
Add-MpPreference -ExclusionPath $(pnpm store path)
```

### On POSIX systems

```sh
curl -fsSL https://get.pnpm.io/install.sh | sh -
```

If you don't have curl installed, you would like to use wget:

```sh
wget -qO- https://get.pnpm.io/install.sh | sh -
```

:::tip

You may use the [pnpm runtime] command then to install Node.js.

:::

### In a Docker container

```sh
# bash
wget -qO- https://get.pnpm.io/install.sh | env ENV="$HOME/.bashrc" SHELL="$(which bash)" bash -
# sh
wget -qO- https://get.pnpm.io/install.sh | env ENV="$HOME/.shrc" SHELL="$(which sh)" sh -
# dash
wget -qO- https://get.pnpm.io/install.sh | env ENV="$HOME/.dashrc" SHELL="$(which dash)" dash -
```

### Installing a specific version

Prior to running the install script, you may optionally set an env variable `PNPM_VERSION` to install a specific version of pnpm:

```sh
curl -fsSL https://get.pnpm.io/install.sh | env PNPM_VERSION=<version> sh -
```

## Using npm

```sh
npx get-pnpm
```

Node.js 22.13 or newer is needed to run this installer, but not to run pnpm afterwards.

:::tip

Do you wanna use pnpm on CI servers? See: [Continuous Integration](./continuous-integration.md).

:::

## Compatibility

Here is a list of past pnpm versions with respective Node.js version support.

| Node.js    | pnpm 8 | pnpm 9 | pnpm 10 | pnpm 11 | pnpm 12 |
|------------|--------|--------|---------|---------|---------|
| Node.js 14 | ❌     | ❌     | ❌      | ❌      | ❌      |
| Node.js 16 | ✔️      | ❌     | ❌      | ❌      | ❌      |
| Node.js 18 | ✔️      | ✔️      | ✔️       | ❌      | ✔️       |
| Node.js 20 | ✔️      | ✔️      | ✔️       | ❌      | ✔️       |
| Node.js 22 | ✔️      | ✔️      | ✔️       | ✔️       | ✔️       |
| Node.js 24 | ✔️      | ✔️      | ✔️       | ✔️       | ✔️       |
| Node.js 26 | ✔️      | ✔️      | ✔️       | ✔️       | ✔️       |

pnpm 12 only needs Node.js when it is installed from npm; the version installed by the standalone script runs without Node.js.

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

To update pnpm, run the [`self-update`] command:

```
pnpm self-update
```

[`self-update`]: ./cli/self-update.md

## Uninstalling pnpm

If you need to remove the pnpm CLI from your system and any files it has written to your disk, see [Uninstalling pnpm].

[Uninstalling pnpm]: ./uninstall.md
[pnpm runtime]: ./cli/runtime.md
