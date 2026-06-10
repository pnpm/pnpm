import { assertRegistryShapedResolution, isGitHostedTarballUrl } from '@pnpm/lockfile.utils'
import { type PackageSnapshot } from '@pnpm/lockfile.types'

function snapshot (resolution: unknown): PackageSnapshot {
  return { resolution } as PackageSnapshot
}

const MISMATCH = expect.objectContaining({ code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH' })

test('rejects a registry-style depPath backed by a git resolution', () => {
  expect(() => {
    assertRegistryShapedResolution('foo@1.0.0', snapshot({
      type: 'git', repo: 'https://example.com/foo.git', commit: 'abc123',
    }))
  }).toThrow(MISMATCH)
})

test('rejects a registry-style depPath backed by a git-hosted tarball resolution', () => {
  expect(() => {
    assertRegistryShapedResolution('foo@1.0.0', snapshot({
      integrity: 'sha512-deadbeef', tarball: 'https://codeload.github.com/org/foo/tar.gz/abc123', gitHosted: true,
    }))
  }).toThrow(MISMATCH)
})

test('rejects a registry-style depPath backed by a directory resolution', () => {
  expect(() => {
    assertRegistryShapedResolution('foo@1.0.0', snapshot({
      type: 'directory', directory: '../foo',
    }))
  }).toThrow(MISMATCH)
})

test('accepts registry-style depPaths with registry and all-registry variations resolutions', () => {
  expect(() => {
    assertRegistryShapedResolution('foo@1.0.0', snapshot({
      integrity: 'sha512-a',
    }))
  }).not.toThrow()
  expect(() => {
    assertRegistryShapedResolution('bar@1.0.0', snapshot({
      type: 'variations',
      variants: [
        { targets: [{ os: 'darwin' }], resolution: { integrity: 'sha512-a' } },
        { targets: [{ os: 'linux' }], resolution: { integrity: 'sha512-b' } },
      ],
    }))
  }).not.toThrow()
})

test('rejects a registry-style depPath whose variations resolution hides a git variant', () => {
  expect(() => {
    assertRegistryShapedResolution('bar@1.0.0', snapshot({
      type: 'variations',
      variants: [
        { targets: [{ os: 'darwin' }], resolution: { integrity: 'sha512-a' } },
        { targets: [{ os: 'linux' }], resolution: { type: 'git', repo: 'https://example.com/bar.git', commit: 'abc123' } },
      ],
    }))
  }).toThrow(MISMATCH)
})

test('does not flag artifact depPaths with non-registry resolutions', () => {
  expect(() => {
    assertRegistryShapedResolution('foo@git+https://example.com/foo.git#abc123', snapshot({
      type: 'git', repo: 'https://example.com/foo.git', commit: 'abc123',
    }))
  }).not.toThrow()
  expect(() => {
    assertRegistryShapedResolution('bar@https://example.com/bar.tgz', snapshot({
      integrity: 'sha512-deadbeef', tarball: 'https://example.com/bar.tgz',
    }))
  }).not.toThrow()
})

test('rejects a registry-style depPath whose git-host tarball clears the gitHosted flag', () => {
  // A tampered lockfile sets a non-truthy gitHosted on a codeload URL to
  // dodge a flag-only check. The URL itself must still flag it.
  for (const gitHosted of [false, 'true', 'false', 0, 1]) {
    expect(() => {
      assertRegistryShapedResolution('foo@1.0.0', snapshot({
        integrity: 'sha512-deadbeef', tarball: 'https://codeload.github.com/org/foo/tar.gz/abc123', gitHosted,
      }))
    }).toThrow(MISMATCH)
  }
})

test('rejects a registry-style depPath with a non-boolean gitHosted flag', () => {
  expect(() => {
    assertRegistryShapedResolution('foo@1.0.0', snapshot({
      integrity: 'sha512-deadbeef', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz', gitHosted: 'true',
    }))
  }).toThrow(MISMATCH)
})

test('rejects a registry-style depPath backed by a non-http(s) tarball URL', () => {
  for (const tarball of ['file:///tmp/evil.tgz', 'ftp://example.com/evil.tgz']) {
    expect(() => {
      assertRegistryShapedResolution('foo@1.0.0', snapshot({
        integrity: 'sha512-deadbeef', tarball,
      }))
    }).toThrow(MISMATCH)
  }
})

test('rejects a registry-style depPath whose tarball escapes the registry host without a scheme', () => {
  for (const tarball of ['//evil.example.com/foo.tgz', '/\\evil.example.com/foo.tgz', '\\\\evil.example.com\\foo.tgz']) {
    expect(() => {
      assertRegistryShapedResolution('foo@1.0.0', snapshot({
        integrity: 'sha512-deadbeef', tarball,
      }))
    }).toThrow(MISMATCH)
  }
})

test('accepts a registry-style depPath whose tarball is an http(s) or registry-relative URL', () => {
  expect(() => {
    assertRegistryShapedResolution('foo@1.0.0', snapshot({
      integrity: 'sha512-deadbeef', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
    }))
  }).not.toThrow()
  // Lockfiles written by older pnpm versions store registry-relative tarball
  // paths, which resolve against the configured registry.
  expect(() => {
    assertRegistryShapedResolution('@mycompany/mypackage@2.0.0', snapshot({
      integrity: 'sha512-deadbeef', tarball: '@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
    }))
  }).not.toThrow()
})

test('rejects a registry-style depPath whose git-host tarball varies the host casing', () => {
  // Hostnames are case-insensitive; an upper-case codeload host paired with
  // gitHosted: false must not pass as registry-shaped.
  expect(() => {
    assertRegistryShapedResolution('foo@1.0.0', snapshot({
      integrity: 'sha512-deadbeef', tarball: 'https://CODELOAD.GITHUB.COM/org/foo/tar.gz/abc123', gitHosted: false,
    }))
  }).toThrow(MISMATCH)
})

test('isGitHostedTarballUrl() matches the known git hosts regardless of casing', () => {
  expect(isGitHostedTarballUrl('https://codeload.github.com/org/foo/tar.gz/abc123')).toBe(true)
  expect(isGitHostedTarballUrl('https://CODELOAD.GITHUB.COM/org/foo/TAR.GZ/abc123')).toBe(true)
  expect(isGitHostedTarballUrl('https://registry.npmjs.org/foo/-/foo-1.0.0.tgz')).toBe(false)
})
