---
id: installation
title: Installation
---

## Using a standalone script

You may install pnpm even if you don't have Node.js installed, using the following scripts.

### On Windows

Using PowerShell:

```powershell
iwr https://get.pnpm.io/install.ps1 -useb | iex
```

### On POSIX systems

```sh
curl -fsSL https://get.pnpm.io/install.sh | sh -
```

If you don't have curl installed, you would like to use wget:

```sh
wget -qO- https://get.pnpm.io/install.sh | sh -
```

### On Alpine Linux

```sh
# bash
wget -qO- https://get.pnpm.io/install.sh | ENV="$HOME/.bashrc" SHELL="$(which bash)" bash -
# sh
wget -qO- https://get.pnpm.io/install.sh | ENV="$HOME/.shrc" SHELL="$(which sh)" sh -
# dash
wget -qO- https://get.pnpm.io/install.sh | ENV="$HOME/.dashrc" SHELL="$(which dash)" dash -
```

### Prerequisites

If you don't use the standalone script to install pnpm, then you need to have Node.js (at least v14) to be installed on your system.

### Installing a specific version

Prior to running the install script, you may optionally set an env variable `PNPM_VERSION` to install a specific version of pnpm:

```sh
curl -fsSL https://get.pnpm.io/install.sh | env PNPM_VERSION=<version> sh -
```

:::tip

You may use the [pnpm env] command then to install Node.js.

:::

## Using Corepack

Since v16.13, Node.js is shipping [Corepack](https://nodejs.org/api/corepack.html) for managing package managers. This is an experimental feature, so you need to enable it by running:

```
corepack enable
```

If you installed Node.js using Homebrew, you'll need to install corepack separately:

```
brew install corepack
```

This will automatically install pnpm on your system. However, it probably won't be the latest version of pnpm. To upgrade it, check what is the [latest pnpm version](https://github.com/pnpm/pnpm/releases/latest) and run:

```
corepack prepare pnpm@<version> --activate
```

With Node.js v16.17 or newer, you may install the `latest` version of pnpm by just specifying the tag:

```
corepack prepare pnpm@latest --activate
```

## Using npm

```sh
npm install -g pnpm
```

## Using Homebrew

If you have the package manager installed, you can install pnpm using the following command:

```
brew install pnpm
```

## Using Scoop

If you have Scoop installed, you can install pnpm using the following command:

```
scoop install nodejs-lts pnpm
```

:::tip

Do you wanna use pnpm on CI servers? See: [Continuous Integration](./continuous-integration.md).

:::

## Compatibility

Here is a list of past pnpm versions with respective Node.js version support.

| Node.js    | pnpm 4 | pnpm 5 | pnpm 6 | pnpm 7 |
|------------|--------|--------|--------|--------|
| Node.js 10 | ✔️      | ✔️      | ❌     | ❌     |
| Node.js 12 | ✔️      | ✔️      | ✔️      | ❌     |
| Node.js 14 | ✔️      | ✔️      | ✔️      | ✔️      |
| Node.js 16 | ?️      | ?️      | ✔️      | ✔️      |
| Node.js 18 | ?️      | ?️      | ✔️      | ✔️      |

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

First, try to find the location of pnpm by running: `which pnpm`. If you're on Windows, run this command in Git Bash.
You'll get the location of the pnpm command, for instance:

```
$ which pnpm
/c/Program Files/nodejs/pnpm
```

Now that you know where the pnpm CLI is, open that directory and remove any pnpm-related files (`pnpm.cmd`, `pnpx.cmd`, `pnpm`, etc).
Once done, install pnpm again and it should work as expected.

## Using a shorter alias

`pnpm` might be hard to type, so you may use a shorter alias like `pn` instead.

#### Adding a permanent alias on POSIX systems

Just put the following line to your `.bashrc`, `.zshrc`, or `config.fish`:

```
alias pn=pnpm
```

#### Adding a permanent alias in Powershell (Windows):

In a Powershell window with admin rights, execute:

```
notepad $profile.AllUsersAllHosts
```

In the `profile.ps1` file that opens, put:

```
set-alias -name pn -value pnpm
```

Save the file and close the window. You may need to close any open Powershell window in order for the alias to take effect.

## Uninstalling pnpm

If you need to remove the pnpm CLI from your system and any files it has written to your disk, see [Uninstalling pnpm].

[Uninstalling pnpm]: ./uninstall.md
[pnpm env]: ./cli/env.md
