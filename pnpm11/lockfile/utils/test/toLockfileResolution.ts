import { expect, test } from '@jest/globals'
import { pkgSnapshotToResolution, toLockfileResolution } from '@pnpm/lockfile.utils'

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

test('drops reconstructible scoped Artifactory tarballs unless explicitly included', () => {
  const registry = 'https://artifactory.example/artifactory/api/npm/npm-virtual/'
  const pkg = { name: '@acme/widget', version: '1.2.3' }
  const resolution = {
    integrity: 'sha512-AAAA',
    tarball: `${registry}@acme/widget/-/@acme/widget-1.2.3.tgz`,
  }

  expect(toLockfileResolution(pkg, resolution, registry)).toEqual({ integrity: resolution.integrity })
  expect(toLockfileResolution(pkg, resolution, registry, false)).toEqual({ integrity: resolution.integrity })
  expect(toLockfileResolution(pkg, resolution, registry, true)).toEqual(resolution)
  expect(pkgSnapshotToResolution('@acme/widget@1.2.3', {
    resolution: toLockfileResolution(pkg, resolution, registry),
  }, { registries: { default: registry } })).toEqual({
    integrity: resolution.integrity,
    tarball: `${registry}@acme/widget/-/widget-1.2.3.tgz`,
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

test('drops an integrity recorded on a git resolution', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    {
      type: 'git',
      repo: 'https://github.com/foo/bar.git',
      commit: 'e63c09e460269b0c535e4c34debf69bb91d57b22',
      integrity: 'sha512-AAAA',
    } as never,
    REGISTRY
  )).toEqual({
    type: 'git',
    repo: 'https://github.com/foo/bar.git',
    commit: 'e63c09e460269b0c535e4c34debf69bb91d57b22',
  })
})

test('keeps a git resolution without an integrity untouched', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    {
      type: 'git',
      repo: 'https://github.com/foo/bar.git',
      commit: 'e63c09e460269b0c535e4c34debf69bb91d57b22',
      path: '/packages/foo',
    },
    REGISTRY
  )).toEqual({
    type: 'git',
    repo: 'https://github.com/foo/bar.git',
    commit: 'e63c09e460269b0c535e4c34debf69bb91d57b22',
    path: '/packages/foo',
  })
})
