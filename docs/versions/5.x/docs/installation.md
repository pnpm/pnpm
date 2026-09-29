---
id: installation
title: Installation
---

## Using a standalone script

```sh
curl -f https://get.pnpm.io/v5.js | node - add --global pnpm
```

On Windows (PowerShell):

```powershell
(Invoke-WebRequest 'https://get.pnpm.io/v5.js' -UseBasicParsing).Content | node - add --global pnpm
```

## Using npm

```sh
npm install -g pnpm
```

## Via npx resolution

```sh
npx pnpm add -g pnpm
```

## Upgrading

Once you have installed pnpm, there is no need to use other package managers to
update it. You can upgrade pnpm using itself, like so:

```sh
pnpm add -g pnpm
```

:::tip

Do you wanna use pnpm on CI servers? See: [Continuous Integration](./continuous-integration.md).

:::

## Compatibility

Here is a list of past pnpm versions with respective Node.js version support.

| Node.js    | pnpm 1 | pnpm 2 | pnpm 3 | pnpm 4 | pnpm 5 | pnpm 6 |
|------------|--------|--------|--------|--------|--------|--------|
| Node.js 4  | ✔️     | ❌    | ❌    | ❌     | ❌     | ❌    |
| Node.js 6  | ✔️     | ✔️    | ❌    | ❌     | ❌     | ❌    |
| Node.js 8  | ✔️     | ✔️    | ✔️    | ❌     | ❌     | ❌    |
| Node.js 10 | ✔️     | ✔️    | ✔️    | ✔️     | ✔️     | ❌    |
| Node.js 12 | ❌     | ❌    | ✔️    | ✔️     | ✔️     | ✔️    |
| Node.js 14 | ❌     | ❌    | ✔️    | ✔️     | ✔️     | ✔️    |
