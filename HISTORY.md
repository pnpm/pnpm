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
[@indexzero]: https://github.com/indexzero
[@rstacruz]: https://github.com/rstacruz
