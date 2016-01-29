## [v0.6.1]
> Jan 30, 2016

- You can now safely rebuild from interrupted `pnpm install`s and pnpm will pick up where it left off.
- Add support for `bundleDependencies`.
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
[#11]: https://github.com/rstacruz/pnpm/issues/11
[@indexzero]: https://github.com/indexzero
