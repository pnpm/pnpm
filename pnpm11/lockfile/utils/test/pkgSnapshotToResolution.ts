import { expect, test } from '@jest/globals'
import { normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import { pkgSnapshotToResolution } from '@pnpm/lockfile.utils'

const GIT_TARBALL = 'https://codeload.github.com/foo/bar/tar.gz/0123456789abcdef0123456789abcdef01234567'
const LEGACY_GIT_TARBALL = 'https://codeload.github.com/kevva/is-negative/tar.gz/0123456789abcdef0123456789abcdef01234567'
const REVISION_INTEGRITY = `sha512-${'A'.repeat(86)}==`

test('pkgSnapshotToResolution() fails closed on a non-string tarball', () => {
  // A tampered lockfile (YAML) could carry a non-string `tarball` that `new URL()` would
  // string-coerce into an attacker-controlled URL.
  expect(() => pkgSnapshotToResolution('foo@1.0.0', {
    resolution: {
      integrity: 'sha512-AAAA',
      tarball: ['https://attacker.example/foo.tgz'],
    },
  } as never, { registriesByScope: { default: 'https://registry.npmjs.org/' } })).toThrow(
    expect.objectContaining({ code: 'ERR_PNPM_INVALID_TARBALL_RESOLUTION' })
  )
})

test('pkgSnapshotToResolution()', () => {
  expect(pkgSnapshotToResolution('foo@1.0.0', {
    resolution: {
      integrity: 'AAAA',
    },
  }, { registriesByScope: { default: 'https://registry.npmjs.org/' } })).toEqual({
    integrity: 'AAAA',
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  })

  expect(pkgSnapshotToResolution('@mycompany/mypackage@2.0.0', {
    resolution: {
      integrity: 'AAAA',
      tarball: '@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
    },
  }, { registriesByScope: { default: 'https://registry.npmjs.org/', '@mycompany': 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/' } })).toEqual({
    integrity: 'AAAA',
    tarball: 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
  })

  expect(pkgSnapshotToResolution('@mycompany/mypackage@2.0.0', {
    resolution: {
      integrity: 'AAAA',
      tarball: '@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
    },
  }, { registriesByScope: { default: 'https://registry.npmjs.org/', '@mycompany': 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local' } })).toEqual({
    integrity: 'AAAA',
    tarball: 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
  })

  expect(pkgSnapshotToResolution('@cdn.sheetjs.com/xlsx-0.18.9/xlsx-0.18.9.tgz', {
    resolution: {
      integrity: 'sha512-CCCC',
      tarball: 'https://cdn.sheetjs.com/xlsx-0.18.9/xlsx-0.18.9.tgz',
    },
  }, { registriesByScope: { default: 'https://registry.npmjs.org/' } })).toEqual({
    integrity: 'sha512-CCCC',
    tarball: 'https://cdn.sheetjs.com/xlsx-0.18.9/xlsx-0.18.9.tgz',
  })

  // Snapshot for a `file:` dependency whose resolution lacks the tarball
  // field — the tarball should be recovered from the depPath.
  expect(pkgSnapshotToResolution('test-package@file:test-package-1.0.0.tgz', {
    resolution: {
      integrity: 'sha512-AAAA',
    },
    version: '1.0.0',
  }, { registriesByScope: { default: 'https://registry.npmjs.org/' } })).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'file:test-package-1.0.0.tgz',
  })
})

test('pkgSnapshotToResolution() converts git-hosted and file: tarball snapshots', () => {
  // The integrity requirement for registry tarballs is enforced by the npm
  // resolver's lockfile verifier, not here; this pure conversion returns
  // git-hosted (commit-anchored) and file: (local) tarballs as-is.
  expect(pkgSnapshotToResolution('foo@https+++github.com+foo+bar', {
    resolution: {
      tarball: GIT_TARBALL,
      gitHosted: true,
    },
  }, { registriesByScope: { default: 'https://registry.npmjs.org/' } })).toEqual({
    tarball: GIT_TARBALL,
    gitHosted: true,
  })

  expect(pkgSnapshotToResolution('is-negative@https+++codeload.github.com+kevva+is-negative+tar.gz+abc', {
    resolution: {
      tarball: LEGACY_GIT_TARBALL,
    },
  }, { registriesByScope: { default: 'https://registry.npmjs.org/' } })).toEqual({
    tarball: LEGACY_GIT_TARBALL,
  })

  // `file:` tarballs are local files; the user already controls the
  // bytes, and the install pipeline may write them without integrity.
  expect(pkgSnapshotToResolution('local-pkg@file:local-pkg-1.0.0.tgz', {
    resolution: {
      tarball: 'file:local-pkg-1.0.0.tgz',
    },
    version: '1.0.0',
  }, { registriesByScope: { default: 'https://registry.npmjs.org/' } })).toEqual({
    tarball: 'file:local-pkg-1.0.0.tgz',
  })
})

test('pkgSnapshotToResolution() reconstructs the tarball of a registry-qualified snapshot from its named registry', () => {
  expect(pkgSnapshotToResolution('foo@work:1.0.0', {
    resolution: {
      integrity: 'sha512-AAAA',
    },
  }, {
    registriesByScope: { default: 'https://registry.npmjs.org/' },
    registriesByPrefix: normalizeRegistriesByPrefix({ work: 'https://npm.enterprise.example.com/' }),
  })).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'https://npm.enterprise.example.com/foo/-/foo-1.0.0.tgz',
  })

  // The built-in gh alias needs no configuration.
  expect(pkgSnapshotToResolution('@acme/private@gh:2.1.0', {
    resolution: {
      integrity: 'sha512-AAAA',
      tarball: 'https://npm.pkg.github.com/download/@acme/private/2.1.0/abcdef',
    },
  }, {
    registriesByScope: { default: 'https://registry.npmjs.org/' },
  })).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'https://npm.pkg.github.com/download/@acme/private/2.1.0/abcdef',
  })
})

test('pkgSnapshotToResolution() fails when a registry-qualified snapshot names an unknown alias', () => {
  expect(() => pkgSnapshotToResolution('foo@work:1.0.0', {
    resolution: {
      integrity: 'sha512-AAAA',
    },
  }, {
    registriesByScope: { default: 'https://registry.npmjs.org/' },
  })).toThrow(
    expect.objectContaining({ code: 'ERR_PNPM_MISSING_NAMED_REGISTRY' })
  )
})

test('pkgSnapshotToResolution() rejects an alias that only exists on Object.prototype', () => {
  // `constructor` matches the alias grammar and would resolve to a truthy
  // inherited function on a plain object literal, sailing past the
  // fail-closed check and reaching the tarball builder as a non-string.
  for (const inherited of ['constructor', 'toString', 'valueOf']) {
    expect(() => pkgSnapshotToResolution(`foo@${inherited}:1.0.0`, {
      resolution: {
        integrity: 'sha512-AAAA',
      },
    }, {
      registriesByScope: { default: 'https://registry.npmjs.org/' },
    })).toThrow(
      expect.objectContaining({ code: 'ERR_PNPM_MISSING_NAMED_REGISTRY' })
    )
  }
})

test('pkgSnapshotToResolution() hydrates a resolution with a revision from the effective registry', () => {
  expect(pkgSnapshotToResolution('@scope/foo@1.0.0', {
    resolution: {
      integrity: REVISION_INTEGRITY,
      revision: 2,
    },
  }, {
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@scope': 'https://registry.example/~main',
    },
  })).toEqual({
    integrity: REVISION_INTEGRITY,
    revision: 2,
    tarball: `https://registry.example/~main/-/tarballs/sha512/${'A'.repeat(86)}`,
  })
})

test('pkgSnapshotToResolution() hydrates an integrity-only resolution from the canonical URL', () => {
  expect(pkgSnapshotToResolution('foo@1.0.0', {
    resolution: {
      integrity: REVISION_INTEGRITY,
    },
  }, {
    registriesByScope: { default: 'https://registry.npmjs.org/' },
  })).toEqual({
    integrity: REVISION_INTEGRITY,
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  })
})

test.each([0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, '1', '01'])(
  'pkgSnapshotToResolution() rejects malformed revision %s',
  (revision) => {
    expect(() => pkgSnapshotToResolution('foo@1.0.0', {
      resolution: {
        integrity: REVISION_INTEGRITY,
        revision,
      } as never,
    }, {
      registriesByScope: { default: 'https://registry.npmjs.org/' },
    })).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_TARBALL_REVISION',
    }))
  }
)

test.each([{}, [], 1, true])(
  'pkgSnapshotToResolution() rejects non-string revision integrity %#',
  (integrity) => {
    expect(() => pkgSnapshotToResolution('foo@1.0.0', {
      resolution: {
        integrity,
        revision: 2,
      } as never,
    }, {
      registriesByScope: { default: 'https://registry.npmjs.org/' },
    })).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_TARBALL_REVISION',
    }))
  }
)

test.each([
  { tarball: 'file:../foo.tgz' },
  { tarball: 'https://codeload.github.com/foo/bar/tar.gz/abc', gitHosted: true },
])('pkgSnapshotToResolution() rejects a revision on a non-registry tarball', (resolution) => {
  expect(() => pkgSnapshotToResolution('foo@1.0.0', {
    resolution: {
      ...resolution,
      integrity: REVISION_INTEGRITY,
      revision: 1,
    },
  }, {
    registriesByScope: { default: 'https://registry.npmjs.org/' },
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_TARBALL_REVISION',
  }))
})
