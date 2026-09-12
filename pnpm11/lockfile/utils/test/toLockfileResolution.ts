import { describe, expect, test } from '@jest/globals'
import { pkgSnapshotToResolution, toLockfileResolution } from '@pnpm/lockfile.utils'

const REGISTRY = 'https://registry.npmjs.org/'
const GIT_TARBALL = 'https://codeload.github.com/foo/bar/tar.gz/0123456789abcdef0123456789abcdef01234567'
const REVISION_INTEGRITY = `sha512-${'A'.repeat(86)}==`
const REVISION_TARBALL = `https://registry.npmjs.org/-/tarballs/sha512/${'A'.repeat(86)}`

test('keeps the tarball when lockfileIncludeTarballUrl is true', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' },
    { registry: REGISTRY, lockfileIncludeTarballUrl: true }
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  })
})

test('drops the tarball for standard registry URLs by default', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' },
    { registry: REGISTRY }
  )).toEqual({
    integrity: 'sha512-AAAA',
  })
})

test('drops the tarball for standard registry URLs when lockfileIncludeTarballUrl is false', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' },
    { registry: REGISTRY, lockfileIncludeTarballUrl: false }
  )).toEqual({
    integrity: 'sha512-AAAA',
  })
})

test('drops a validated integrity-addressed registry tarball URL', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: REVISION_INTEGRITY, revision: 1, tarball: REVISION_TARBALL },
    { registry: REGISTRY }
  )).toEqual({
    integrity: REVISION_INTEGRITY,
    revision: 1,
  })
})

test('drops a revision URL even when lockfileIncludeTarballUrl is true', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: REVISION_INTEGRITY, revision: 1, tarball: REVISION_TARBALL },
    { registry: REGISTRY, lockfileIncludeTarballUrl: true }
  )).toEqual({
    integrity: REVISION_INTEGRITY,
    revision: 1,
  })
})

test('rejects a revision without an integrity-addressed URL', () => {
  expect(() => toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: REVISION_INTEGRITY, revision: 1 } as never,
    { registry: REGISTRY }
  )).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_TARBALL_REVISION',
  }))
})

test('rejects a revision whose URL registry or digest does not match', () => {
  expect(() => toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: REVISION_INTEGRITY, revision: 1, tarball: `https://attacker.example/-/tarballs/sha512/${'A'.repeat(86)}` },
    { registry: REGISTRY }
  )).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_TARBALL_REVISION',
  }))
  expect(() => toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: REVISION_INTEGRITY, revision: 1, tarball: `https://registry.npmjs.org/-/tarballs/sha512/${'B'.repeat(86)}` },
    { registry: REGISTRY }
  )).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_TARBALL_REVISION',
  }))
})

test('normalizes an original served from the digest route to integrity only', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: REVISION_INTEGRITY, tarball: REVISION_TARBALL },
    { registry: REGISTRY, lockfileIncludeTarballUrl: true }
  )).toEqual({
    integrity: REVISION_INTEGRITY,
  })
})

test.each([0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, '1', '01'])('rejects malformed revision %s', (revision) => {
  expect(() => toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: REVISION_INTEGRITY, revision, tarball: REVISION_TARBALL } as never,
    { registry: REGISTRY }
  )).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_TARBALL_REVISION',
  }))
})

test('keeps the tarball for non-standard registry URLs when lockfileIncludeTarballUrl is false', () => {
  // A tarball URL whose host doesn't match the configured registry cannot be
  // reconstructed from name+version+registry, so dropping it would break
  // re-fetching on `--frozen-lockfile`. `lockfileIncludeTarballUrl: false`
  // only suppresses URLs that *can* be reconstructed.
  expect(toLockfileResolution(
    { name: 'esprima-fb', version: '3001.1.0-dev-harmony-fb' },
    { integrity: 'sha512-AAAA', tarball: 'https://example.com/esprima-fb/-/esprima-fb-3001.1.0-dev-harmony-fb.tgz' },
    { registry: REGISTRY, lockfileIncludeTarballUrl: false }
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'https://example.com/esprima-fb/-/esprima-fb-3001.1.0-dev-harmony-fb.tgz',
  })
})

test.each([
  'https://npm.example.com/@babel%2Fcore/-/core-7.0.0.tgz',
  'https://npm.example.com/@babel%2fcore/-/core-7.0.0.tgz',
])('keeps a scoped tarball URL that percent-encodes the scope separator: %s', (tarball) => {
  expect(toLockfileResolution(
    { name: '@babel/core', version: '7.0.0' },
    { integrity: 'sha512-AAAA', tarball },
    { registry: 'https://npm.example.com/', lockfileIncludeTarballUrl: false }
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball,
  })
})

test.each([
  'https://registry.npmjs.org/@babel%2Fcore/-/core-7.0.0.tgz',
  'https://registry.npmjs.org/@babel%2fcore/-/core-7.0.0.tgz',
])('drops a percent-encoded scoped tarball URL on the public registry: %s', (tarball) => {
  expect(toLockfileResolution(
    { name: '@babel/core', version: '7.0.0' },
    { integrity: 'sha512-AAAA', tarball },
    { registry: REGISTRY, lockfileIncludeTarballUrl: false }
  )).toEqual({
    integrity: 'sha512-AAAA',
  })
})

test('keeps GitHub Packages /download/ tarball URLs when lockfileIncludeTarballUrl is false', () => {
  // GitHub Packages serves tarballs at /download/<scope>/<name>/<version>/<hash>,
  // which cannot be derived from name+version+registry. See
  // https://github.com/pnpm/pnpm/issues/11276.
  expect(toLockfileResolution(
    { name: '@example/private', version: '1.2.3' },
    { integrity: 'sha512-AAAA', tarball: 'https://npm.pkg.github.com/download/@example/private/1.2.3/0123456789abcdef0123456789abcdef01234567' },
    { registry: 'https://npm.pkg.github.com/', lockfileIncludeTarballUrl: false }
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
    { registry: REGISTRY, lockfileIncludeTarballUrl: false }
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'file:test-package-1.0.0.tgz',
  })
})

test('keeps file: tarballs even when lockfileIncludeTarballUrl is undefined', () => {
  expect(toLockfileResolution(
    { name: 'test-package', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: 'file:test-package-1.0.0.tgz' },
    { registry: REGISTRY }
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'file:test-package-1.0.0.tgz',
  })
})

test('keeps git-hosted tarballs when lockfileIncludeTarballUrl is false', () => {
  expect(toLockfileResolution(
    { name: 'foo', version: '1.0.0' },
    { integrity: 'sha512-AAAA', tarball: GIT_TARBALL },
    { registry: REGISTRY, lockfileIncludeTarballUrl: false }
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
    { registry: REGISTRY, lockfileIncludeTarballUrl: false }
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
    { registry: REGISTRY, lockfileIncludeTarballUrl: true }
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
    { registry: REGISTRY, lockfileIncludeTarballUrl: true }
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
    { registry: REGISTRY }
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
    { registry: REGISTRY }
  )).toEqual({
    type: 'git',
    repo: 'https://github.com/foo/bar.git',
    commit: 'e63c09e460269b0c535e4c34debf69bb91d57b22',
    path: '/packages/foo',
  })
})

describe('on a registry declared as Artifactory', () => {
  const registry = 'https://artifactory.example/artifactory/api/npm/npm-virtual/'
  const registryOptionsByUrl = { [registry]: { serverType: 'artifactory' as const } }
  const pkg = { name: '@acme/widget', version: '1.2.3' }
  const tarball = `${registry}@acme/widget/-/@acme/widget-1.2.3.tgz`

  test('drops the scoped tarball URL it advertises', () => {
    expect(toLockfileResolution(
      pkg,
      { integrity: 'sha512-AAAA', tarball },
      { registry, serverType: 'artifactory' }
    )).toEqual({
      integrity: 'sha512-AAAA',
    })
  })

  // The invariant the whole setting rests on: whatever the writer omits, the
  // reader has to rebuild byte for byte, or a frozen install fetches a URL the
  // registry never served.
  test('rebuilds the dropped URL exactly', () => {
    const lockfileResolution = toLockfileResolution(
      pkg,
      { integrity: 'sha512-AAAA', tarball },
      { registry, serverType: 'artifactory' }
    )
    expect(pkgSnapshotToResolution('@acme/widget@1.2.3', { resolution: lockfileResolution }, {
      registriesByScope: { default: registry },
      registryOptionsByUrl,
    })).toEqual({ integrity: 'sha512-AAAA', tarball })
  })

  test('keeps the npm-layout URL, which this registry does not serve', () => {
    const npmLayoutTarball = `${registry}@acme/widget/-/widget-1.2.3.tgz`
    expect(toLockfileResolution(
      pkg,
      { integrity: 'sha512-AAAA', tarball: npmLayoutTarball },
      { registry, serverType: 'artifactory' }
    )).toEqual({
      integrity: 'sha512-AAAA',
      tarball: npmLayoutTarball,
    })
  })

  test('keeps the scoped tarball URL when lockfileIncludeTarballUrl is true', () => {
    expect(toLockfileResolution(
      pkg,
      { integrity: 'sha512-AAAA', tarball },
      { registry, serverType: 'artifactory', lockfileIncludeTarballUrl: true }
    )).toEqual({
      integrity: 'sha512-AAAA',
      tarball,
    })
  })
})

test('keeps the Artifactory-shaped URL of a registry that was not declared as Artifactory', () => {
  const registry = 'https://artifactory.example/artifactory/api/npm/npm-virtual/'
  const tarball = `${registry}@acme/widget/-/@acme/widget-1.2.3.tgz`
  expect(toLockfileResolution(
    { name: '@acme/widget', version: '1.2.3' },
    { integrity: 'sha512-AAAA', tarball },
    { registry }
  )).toEqual({
    integrity: 'sha512-AAAA',
    tarball,
  })
})
