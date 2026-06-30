import { expect, test } from '@jest/globals'
import { toLockfileResolution } from '@pnpm/lockfile.utils'

const REGISTRY = 'https://registry.npmjs.org/'
const GIT_TARBALL = 'https://codeload.github.com/foo/bar/tar.gz/0123456789abcdef0123456789abcdef01234567'

test('keeps the tarball when lockfileIncludeTarballUrl is true', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' },
    REGISTRY,
    true
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  })
})

test('drops the tarball for standard registry URLs by default', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' },
    REGISTRY
  )).toEqual({
    integrity: 'sha512-AAAA',
  })
})

test('drops the tarball for standard registry URLs when lockfileIncludeTarballUrl is false', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' },
    REGISTRY,
    false
  )).toEqual({
    integrity: 'sha512-AAAA',
  })
})

test('keeps the tarball for non-standard registry URLs when lockfileIncludeTarballUrl is false', () => {
  // A tarball URL whose host doesn't match the configured registry cannot be
  // reconstructed from name+version+registry, so dropping it would break
  // re-fetching on `--frozen-lockfile`. `lockfileIncludeTarballUrl: false`
  // only suppresses URLs that *can* be reconstructed.
  expect(toLockfileResolution(
    { name: 'esprima-fb', version: '3001.1.0-dev-harmony-fb' },
    { integrity: 'sha512-AAAA', tarball: 'https://example.com/esprima-fb/-/esprima-fb-3001.1.0-dev-harmony-fb.tgz' },
    REGISTRY,
    false
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'https://example.com/esprima-fb/-/esprima-fb-3001.1.0-dev-harmony-fb.tgz',
  })
})

test('keeps GitHub Packages /download/ tarball URLs when lockfileIncludeTarballUrl is false', () => {
  // GitHub Packages serves tarballs at /download/<scope>/<name>/<version>/<hash>,
  // which cannot be derived from name+version+registry. See
  // https://github.com/pnpm/pnpm/issues/11276.
  expect(toLockfileResolution(
    { name: '@example/private', version: '1.2.3' },
    { integrity: 'sha512-AAAA', tarball: 'https://npm.pkg.github.com/download/@example/private/1.2.3/0123456789abcdef0123456789abcdef01234567' },
    'https://npm.pkg.github.com/',
    false
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'https://npm.pkg.github.com/download/@example/private/1.2.3/0123456789abcdef0123456789abcdef01234567',
  })
})

test('keeps file: tarballs even when lockfileIncludeTarballUrl is false', () => {
  // file: tarballs cannot be reconstructed from name+version+registry, so the
  // tarball field must remain so the package can be re-fetched on install.
  expect(toLockfileResolution(
    { name: 'test-package', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'file:test-package-1.0.0.tgz' },
    REGISTRY,
    false
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'file:test-package-1.0.0.tgz',
  })
})

test('keeps file: tarballs even when lockfileIncludeTarballUrl is undefined', () => {
  expect(toLockfileResolution(
    { name: 'test-package', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'file:test-package-1.0.0.tgz' },
    REGISTRY
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'file:test-package-1.0.0.tgz',
  })
})

test('keeps git-hosted tarballs when lockfileIncludeTarballUrl is false', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: GIT_TARBALL },
    REGISTRY,
    false
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: GIT_TARBALL,
    gitHosted: true,
  })
})

test('keeps the path of a git-hosted tarball pointing to a subdirectory', () => {
  // The path selects the subdirectory to extract from a monorepo tarball
  // (`repo#commit&path:/sub/dir`). Dropping it makes later installs silently
  // unpack the repository root. See https://github.com/pnpm/pnpm/issues/12304.
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: GIT_TARBALL, gitHosted: true, path: '/packages/foo' },
    REGISTRY,
    false
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: GIT_TARBALL,
    gitHosted: true,
    path: '/packages/foo',
  })
})

test('keeps the path of a git-hosted tarball when lockfileIncludeTarballUrl is true', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: GIT_TARBALL, gitHosted: true, path: '/packages/foo' },
    REGISTRY,
    true
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: GIT_TARBALL,
    gitHosted: true,
    path: '/packages/foo',
  })
})

test('records gitHosted on the lockfile entry when set on the resolution', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: GIT_TARBALL, gitHosted: true },
    REGISTRY,
    true
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: GIT_TARBALL,
    gitHosted: true,
  })
})
