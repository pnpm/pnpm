[简体中文](https://pnpm.io/zh/) |
[日本語](https://pnpm.io/ja/) |
[한국어](https://pnpm.io/ko/) |
[Italiano](https://pnpm.io/it/) |
[Português Brasileiro](https://pnpm.io/pt/)

![](https://i.imgur.com/qlW1eEG.png)

Fast, disk space efficient package manager:

* **Fast.** Up to 2x faster than the alternatives (see [benchmark](#benchmark)).
* **Efficient.** Files inside `node_modules` are linked from a single content-addressable storage.
* **[Great for monorepos](https://pnpm.io/workspaces).**
* **Strict.** A package can access only dependencies that are specified in its `package.json`.
* **Deterministic.** Has a lockfile called `pnpm-lock.yaml`.
* **Works as a Node.js version manager.** See [pnpm env use](https://pnpm.io/cli/env).
* **Works everywhere.** Supports Windows, Linux, and macOS.
* **Battle-tested.** Used in production by teams of [all sizes](https://pnpm.io/users) since 2016.
* [See the full feature comparison with npm and Yarn](https://pnpm.io/feature-comparison).

To quote the [Rush](https://rushjs.io/) team:

> Microsoft uses pnpm in Rush repos with hundreds of projects and hundreds of PRs per day, and we’ve found it to be very fast and reliable.

[![npm version](https://img.shields.io/npm/v/pnpm.svg?label=latest)](https://github.com/pnpm/pnpm/releases/latest)
[![Join the chat at Discord](https://img.shields.io/discord/731599538665553971.svg)](https://r.pnpm.io/chat)
[![OpenCollective](https://opencollective.com/pnpm/backers/badge.svg)](https://opencollective.com/pnpm)
[![OpenCollective](https://opencollective.com/pnpm/sponsors/badge.svg)](https://opencollective.com/pnpm)
[![Twitter Follow](https://img.shields.io/twitter/follow/pnpmjs.svg?style=social&label=Follow)](https://twitter.com/intent/follow?screen_name=pnpmjs&region=follow_link)
[![Stand With Ukraine](https://raw.githubusercontent.com/vshymanskyy/StandWithUkraine/main/badges/StandWithUkraine.svg)](https://stand-with-ukraine.pp.ua)

## Platinum Sponsors

<table>
  <tbody>
    <tr>
      <td align="center" valign="middle">
        <a href="https://bit.dev/?utm_source=pnpm&utm_medium=readme" target="_blank"><img src="https://pnpm.io/img/users/bit.svg" width="80"></a>
      </td>
      <td align="center" valign="middle">
        <a href="https://figma.com/?utm_source=pnpm&utm_medium=readme" target="_blank"><img src="https://pnpm.io/img/users/figma.svg" width="80"></a>
      </td>
    </tr>
  </tbody>
</table>

## Gold Sponsors

<table>
  <tbody>
    <tr>
      <td align="center" valign="middle">
        <a href="https://discord.com/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/discord.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/discord_light.svg" />
            <img src="https://pnpm.io/img/users/discord.svg" width="220" />
          </picture>
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://prisma.io/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/prisma.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/prisma_light.svg" />
            <img src="https://pnpm.io/img/users/prisma.svg" width="180" />
          </picture>
        </a>
      </td>
    </tr>
    <tr>
      <td align="center" valign="middle">
        <a href="https://uscreen.de/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/uscreen.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/uscreen_light.svg" />
            <img src="https://pnpm.io/img/users/uscreen.svg" width="180" />
          </picture>
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://www.jetbrains.com/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/jetbrains.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/jetbrains.svg" />
            <img src="https://pnpm.io/img/users/jetbrains.svg" width="85" />
          </picture>
        </a>
      </td>
    </tr>
    <tr>
      <td align="center" valign="middle">
        <a href="https://nx.dev/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/nx.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/nx_light.svg" />
            <img src="https://pnpm.io/img/users/nx.svg" width="120" />
          </picture>
        </a>
      </td>
    </tr>
  </tbody>
</table>

## Silver Sponsors

<table>
  <tbody>
    <tr>
      <td align="center" valign="middle">
        <a href="https://leniolabs.com/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <img src="https://pnpm.io/img/users/leniolabs.jpg" width="80">
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://vercel.com/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/vercel.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/vercel_light.svg" />
            <img src="https://pnpm.io/img/users/vercel.svg" width="180" />
          </picture>
        </a>
      </td>
    </tr>
    <tr>
      <td align="center" valign="middle">
        <a href="https://depot.dev/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/depot.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/depot_light.svg" />
            <img src="https://pnpm.io/img/users/depot.svg" width="200" />
          </picture>
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://moonrepo.dev/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/moonrepo.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/moonrepo_light.svg" />
            <img src="https://pnpm.io/img/users/moonrepo.svg" width="200" />
          </picture>
        </a>
      </td>
    </tr>
    <tr>
      <td align="center" valign="middle">
        <a href="https://www.thinkmill.com.au/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/thinkmill.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/thinkmill_light.svg" />
            <img src="https://pnpm.io/img/users/thinkmill.svg" width="200" />
          </picture>
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://devowl.io/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/devowlio.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/devowlio.svg" />
            <img src="https://pnpm.io/img/users/devowlio.svg" width="200" />
          </picture>
        </a>
      </td>
    </tr>
    <tr>
      <td align="center" valign="middle">
        <a href="https://macpaw.com/?utm_source=pnpm&utm_medium=readme" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/macpaw.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/macpaw_light.svg" />
            <img src="https://pnpm.io/img/users/macpaw.svg" width="200" />
          </picture>
        </a>
      </td>
    </tr>
  </tbody>
</table>

Support this project by [becoming a sponsor](https://opencollective.com/pnpm#sponsor).

## Background

pnpm uses a content-addressable filesystem to store all files from all module directories on a disk.
When using npm, if you have 100 projects using lodash, you will have 100 copies of lodash on disk.
With pnpm, lodash will be stored in a content-addressable storage, so:

1. If you depend on different versions of lodash, only the files that differ are added to the store.
  If lodash has 100 files, and a new version has a change only in one of those files,
  `pnpm update` will only add 1 new file to the storage.
1. All the files are saved in a single place on the disk. When packages are installed, their files are linked
  from that single place consuming no additional disk space. Linking is performed using either hard-links or reflinks (copy-on-write).

As a result, you save gigabytes of space on your disk and you have a lot faster installations!
If you'd like more details about the unique `node_modules` structure that pnpm creates and
why it works fine with the Node.js ecosystem, read this small article: [Flat node_modules is not the only way](https://pnpm.io/blog/2020/05/27/flat-node-modules-is-not-the-only-way).

💖 Like this project? Let people know with a [tweet](https://r.pnpm.io/tweet)

## Installation

For installation options [visit our website](https://pnpm.io/installation).

## Usage

Just use pnpm in place of npm/Yarn. E.g., install dependencies via:

```
pnpm install
```

For more advanced usage, read [pnpm CLI](https://pnpm.io/pnpm-cli) on our website, or run `pnpm help`.

## Benchmark

pnpm is up to 2x faster than npm and Yarn classic. See all benchmarks [here](https://r.pnpm.io/benchmarks).

Benchmarks on an app with lots of dependencies:

![](https://pnpm.io/img/benchmarks/alotta-files.svg)

## Support

- [Frequently Asked Questions](https://pnpm.io/faq)
- [Chat](https://r.pnpm.io/chat)
- [Twitter](https://twitter.com/pnpmjs)

## Backers

Thank you to all our backers! [Become a backer](https://opencollective.com/pnpm#backer)

<a href="https://opencollective.com/pnpm#backers" target="_blank"><img src="https://opencollective.com/pnpm/backers.svg?width=890"></a>

## Contributors

This project exists thanks to all the people who contribute. [Contribute](../../blob/main/CONTRIBUTING.md).

<a href="../../graphs/contributors"><img src="https://opencollective.com/pnpm/contributors.svg?width=890&button=false" /></a>

## License

[MIT](https://github.com/pnpm/pnpm/blob/main/LICENSE)

