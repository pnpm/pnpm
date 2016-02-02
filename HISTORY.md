## [v0.17.0]
> Unreleased

- [#45] - Improve support for 3rd-party registries like GemFury. ([@misterbyrne])
- [#76] - Fix `pnpm --help`. ([@asbjornenge])

[v0.17.0]: https://github.com/rstacruz/pnpm/compare/v0.16.0...v0.17.0

## [v0.16.0]
> Feb  2, 2016

- [#74] - The package has been renamed from `pnpm.js` to `pnpm`. Update accordingly!
- No functional changes since v0.15.0.

[v0.16.0]: https://github.com/rstacruz/pnpm/compare/v0.15.0...v0.16.0

## [v0.15.0]
> Feb  2, 2016

- [#72] - Expose a programatic API via `require('pnpm')` (not documented for now).
- [#73] - Fix Node v0.12 compatibility.

[v0.15.0]: https://github.com/rstacruz/pnpm/compare/v0.14.0...v0.15.0

## [v0.14.0]
> Feb  1, 2016

- [#6], [#64] - Experimental Windows support.
- Documentation is now available at <http://ricostacruz.com/pnpm>.

[v0.14.0]: https://github.com/rstacruz/pnpm/compare/v0.13.0...v0.14.0

## [v0.13.0]
> Feb 1, 2016

- **Semi-breaking** - store format was slightly changed (to support peer dependencies, [#46]). pnpm will continue to be compatible with the old store format, but rebuilding `node_modules` is recommended to take advantage of new features.
- [#41] - Support `--save`, `--save-dev`, `--save-optional` and `--save-exact`. ([@davej])
- [#46] - Support modules being able to `require()` their peer dependencies (aka, emulate npm3-style flatness)
- [#57] - Support simpler Windows terminals.
- Improve logging appearance (show which packages are being fetched and which are queued).

[v0.13.0]: https://github.com/rstacruz/pnpm/compare/v0.12.1...v0.13.0

## [v0.12.0]
> Jan 31, 2016

- **Semi-breaking** - store format was slightly changed. pnpm will continue to be compatible with the old store format, but rebuilding `node_modules` is recommended to take advantage of new features.
- [#38] - Allow compatibility with npm utilities:
  - npm dedupe
  - npm shrinkwrap
  - npm prune
  - npm ls
  - npm rebuild

[v0.12.0]: https://github.com/rstacruz/pnpm/compare/v0.11.1...v0.12.0

## [v0.11.1]
> Jan 31, 2016

- [#9], [#36] - Support exact versions in scoped packages (`pnpm i @rstacruz/tap-spec@4.1.1`).

[v0.11.1]: https://github.com/rstacruz/pnpm/compare/v0.11.0...v0.11.1

## [v0.11.0]
> Jan 31, 2016

- [#24], [#34] - Limit download concurrency.
- [#10], [#35] - Allow custom registries (`npm config set registry http://npmjs.eu`).

[v0.11.0]: https://github.com/rstacruz/pnpm/compare/v0.10.1...v0.11.0

## [v0.10.1]
> Jan 30, 2016

- [#33] - Fix instances of running pnpm in directories without package.json.

[v0.10.1]: https://github.com/rstacruz/pnpm/compare/v0.10.0...v0.10.1

## [v0.10.0]
> Jan 30, 2016

- [#32] - Install from tarballs (`pnpm i http://site.com/package.tgz`).

[v0.10.0]: https://github.com/rstacruz/pnpm/compare/v0.9.0...v0.10.0

## [v0.9.0]
> Jan 30, 2016

- [#31] - Improve `bundleDependencies` support.

[v0.9.0]: https://github.com/rstacruz/pnpm/compare/v0.8.2...v0.9.0

## [v0.8.2]
> Jan 30, 2016

- Fix using `pnpm install` from dirs without package.json.

[v0.8.2]: https://github.com/rstacruz/pnpm/compare/v0.8.1...v0.8.2

## [v0.8.1]
> Jan 30, 2016

- [#25] - Improve `node-gyp` support (with preliminary Windows support).

[v0.8.1]: https://github.com/rstacruz/pnpm/compare/v0.8.0...v0.8.1

## [v0.8.0]
> Jan 30, 2016

- [#25] - Support `node-gyp`.

[v0.8.0]: https://github.com/rstacruz/pnpm/compare/v0.7.0...v0.8.0

## [v0.7.0]
> Jan 30, 2016

- [#18] - Support lifecycle hooks and compiled modules.

[v0.7.0]: https://github.com/rstacruz/pnpm/compare/v0.6.1...v0.7.0

## [v0.6.1]
> Jan 30, 2016

- [#17] - You can now safely rebuild from interrupted `pnpm install`s and pnpm will pick up where it left off.
- [#16] - Add support for `bundleDependencies`.
- Throw errors when doing `pnpm i github/repo`—this isn't supported yet (but will be!).

[v0.6.1]: https://github.com/rstacruz/pnpm/compare/v0.5.0...v0.6.1

## [v0.5.0]
> Jan 30, 2016

- Support scoped modules without version spec (eg, `pnpm i @rstacruz/tap-spec`).
- Lots of internal cleanups.

[v0.5.0]: https://github.com/rstacruz/pnpm/compare/v0.4.1...v0.5.0

## [v0.4.1]
> Jan 29, 2016

- [#11] - Fix using multiple scoped modules from the same scope.

[v0.4.1]: https://github.com/rstacruz/pnpm/compare/v0.4.0...v0.4.1

## [v0.4.0]
> Jan 29, 2016

- [#9] - Add preliminary support for scoped modules. ([#11], [@indexzero])

[v0.4.0]: https://github.com/rstacruz/pnpm/compare/v0.3.0...v0.4.0

## [v0.3.0]
> Jan 29, 2016

- Warn on unsupported flags (eg, `pnpm i --save x`).
- Cleanups and refactoring.

[v0.3.0]: https://github.com/rstacruz/pnpm/compare/v0.2.2...v0.3.0

## [v0.2.2]
> Jan 28, 2016

- Fix dependency problems.

[v0.2.2]: https://github.com/rstacruz/pnpm/compare/v0.2.1...v0.2.2

## [v0.2.1]
> Jan 28, 2016

- Fix "can't find module 'debug'" error.

[v0.2.1]: https://github.com/rstacruz/pnpm/compare/v0.2.0...v0.2.1

## [v0.2.0]
> Jan 28, 2016

- Improve atomicness of operations.
- Symlink bins into `node_modules/.bin`.

[v0.2.0]: https://github.com/rstacruz/pnpm/compare/v0.1.0...v0.2.0

## [v0.1.0]
> Jan 28, 2016

- Initial preview release.

[v0.1.0]: https://github.com/rstacruz/pnpm/blob/v0.1.0
[#6]: https://github.com/rstacruz/pnpm/issues/6
[#9]: https://github.com/rstacruz/pnpm/issues/9
[#10]: https://github.com/rstacruz/pnpm/issues/10
[#11]: https://github.com/rstacruz/pnpm/issues/11
[#16]: https://github.com/rstacruz/pnpm/issues/16
[#17]: https://github.com/rstacruz/pnpm/issues/17
[#18]: https://github.com/rstacruz/pnpm/issues/18
[#24]: https://github.com/rstacruz/pnpm/issues/24
[#25]: https://github.com/rstacruz/pnpm/issues/25
[#31]: https://github.com/rstacruz/pnpm/issues/31
[#32]: https://github.com/rstacruz/pnpm/issues/32
[#33]: https://github.com/rstacruz/pnpm/issues/33
[#34]: https://github.com/rstacruz/pnpm/issues/34
[#35]: https://github.com/rstacruz/pnpm/issues/35
[#36]: https://github.com/rstacruz/pnpm/issues/36
[#38]: https://github.com/rstacruz/pnpm/issues/38
[#41]: https://github.com/rstacruz/pnpm/issues/41
[#45]: https://github.com/rstacruz/pnpm/issues/45
[#46]: https://github.com/rstacruz/pnpm/issues/46
[#57]: https://github.com/rstacruz/pnpm/issues/57
[#64]: https://github.com/rstacruz/pnpm/issues/64
[#72]: https://github.com/rstacruz/pnpm/issues/72
[#73]: https://github.com/rstacruz/pnpm/issues/73
[#74]: https://github.com/rstacruz/pnpm/issues/74
[#76]: https://github.com/rstacruz/pnpm/issues/76
[@asbjornenge]: https://github.com/asbjornenge
[@davej]: https://github.com/davej
[@indexzero]: https://github.com/indexzero
[@misterbyrne]: https://github.com/misterbyrne
[@rstacruz]: https://github.com/rstacruz
