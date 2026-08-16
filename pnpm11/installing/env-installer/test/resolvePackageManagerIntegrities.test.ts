import { expect, jest, test } from '@jest/globals'
import { isPackageManagerResolved } from '@pnpm/installing.env-installer'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'

const resolveManifestDependencies = jest.fn<() => Promise<LockfileObject>>()
jest.unstable_mockModule('../src/resolveManifestDependencies.js', () => ({
  resolveManifestDependencies,
}))
const { resolvePackageManagerIntegrities } = await import('../src/resolvePackageManagerIntegrities.js')

test('the JS pnpm and @pnpm/exe are both pinned for the majors that publish them separately', () => {
  expect(isPackageManagerResolved(envLockfile({ 'pnpm': '11.0.0', '@pnpm/exe': '11.0.0' }), '11.0.0')).toBe(true)
  expect(isPackageManagerResolved(envLockfile({ pnpm: '11.0.0' }), '11.0.0')).toBe(false)
  expect(isPackageManagerResolved(envLockfile({ 'pnpm': '6.17.1', '@pnpm/exe': '6.17.1' }), '6.17.1')).toBe(true)
})

test('only pnpm is pinned from v12, where the unscoped pnpm is itself the executable', () => {
  expect(isPackageManagerResolved(envLockfile({ pnpm: '12.0.0' }), '12.0.0')).toBe(true)
  expect(isPackageManagerResolved(envLockfile({ 'pnpm': '12.0.0', '@pnpm/exe': '12.0.0' }), '12.0.0')).toBe(false)
})

test('only pnpm is pinned before @pnpm/exe was published', () => {
  expect(isPackageManagerResolved(envLockfile({ pnpm: '6.16.0' }), '6.16.0')).toBe(true)
})

test('an env lockfile pinning another pnpm version is not resolved', () => {
  expect(isPackageManagerResolved(envLockfile({ pnpm: '12.0.0' }), '12.1.0')).toBe(false)
  expect(isPackageManagerResolved(undefined, '12.0.0')).toBe(false)
})

// A registry that advertises tarballs on another host (a load-balanced proxy
// or Artifactory-style mirror) must still yield integrity-only
// package-manager entries, or the bootstrap validation rejects them.
// See https://github.com/pnpm/pnpm/issues/13619.
test('registry tarball URLs are dropped from package-manager resolutions; file:, git-hosted, and subdir tarballs are kept', async () => {
  resolveManifestDependencies.mockResolvedValueOnce({
    lockfileVersion: '9.0',
    importers: {
      '.': {
        specifiers: { pnpm: '12.0.0' },
        dependencies: { pnpm: '12.0.0' },
      },
    },
    packages: {
      'pnpm@12.0.0': {
        resolution: {
          integrity: 'sha512-pnpm',
          tarball: 'https://mirror-pool-7.example.com/registry/pnpm/-/pnpm-12.0.0.tgz',
        },
        dependencies: {
          'git-hosted-dep': '1.0.0',
          'local-dep': '1.0.0',
          'subdir-dep': '1.0.0',
        },
      },
      'git-hosted-dep@1.0.0': {
        resolution: {
          integrity: 'sha512-git',
          tarball: 'https://codeload.github.com/org/repo/tar.gz/abc',
          gitHosted: true,
        },
      },
      'local-dep@1.0.0': {
        resolution: {
          integrity: 'sha512-local',
          tarball: 'file:../local-dep.tgz',
        },
      },
      'subdir-dep@1.0.0': {
        resolution: {
          integrity: 'sha512-subdir',
          tarball: 'https://codeload.github.com/org/mono/tar.gz/def',
          path: '/packages/a',
        },
      },
    },
  } as unknown as LockfileObject)

  const result = await resolvePackageManagerIntegrities('12.0.0', {
    registriesByScope: { default: 'https://mirror.example.com/' },
    rootDir: '/repo',
    storeController: {} as never,
    storeDir: '/store',
    save: false,
  })

  const resolutionOf = (depPath: string) =>
    (result.packages as Record<string, { resolution: unknown }>)[depPath].resolution
  expect(resolutionOf('pnpm@12.0.0')).toEqual({ integrity: 'sha512-pnpm' })
  expect(resolutionOf('git-hosted-dep@1.0.0')).toEqual({
    integrity: 'sha512-git',
    tarball: 'https://codeload.github.com/org/repo/tar.gz/abc',
    gitHosted: true,
  })
  expect(resolutionOf('local-dep@1.0.0')).toEqual({
    integrity: 'sha512-local',
    tarball: 'file:../local-dep.tgz',
  })
  expect(resolutionOf('subdir-dep@1.0.0')).toEqual({
    integrity: 'sha512-subdir',
    tarball: 'https://codeload.github.com/org/mono/tar.gz/def',
    path: '/packages/a',
  })
})

function envLockfile (packageManagerDependencies: Record<string, string>): EnvLockfile {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        configDependencies: {},
        packageManagerDependencies: Object.fromEntries(
          Object.entries(packageManagerDependencies).map(([name, version]) => [name, { specifier: version, version }])
        ),
      },
    },
    packages: {},
    snapshots: {},
  } as unknown as EnvLockfile
}
