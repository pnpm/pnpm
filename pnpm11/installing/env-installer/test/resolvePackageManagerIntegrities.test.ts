import { expect, jest, test } from '@jest/globals'
import { isPackageManagerResolved } from '@pnpm/installing.env-installer'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import type { ProjectManifest } from '@pnpm/types'

const resolveManifestDependencies =
  jest.fn<(manifest: ProjectManifest, opts: unknown) => Promise<LockfileObject>>()
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
test('an entry recording the wanted version under a stale specifier is not resolved', () => {
  // Changing an exact pin to a range that still includes the recorded version
  // leaves the version alone but not the specifier, so the entry still has to
  // be rewritten.
  const lockfile = envLockfile({ pnpm: '12.0.0' })
  expect(isPackageManagerResolved(lockfile, '12.0.0')).toBe(true)
  expect(isPackageManagerResolved(lockfile, '12.0.0', '^12.0.0')).toBe(false)
})

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

test('a lockfile that pins another version is not updated under frozenLockfile', async () => {
  await expect(
    resolvePackageManagerIntegrities('12.0.0', {
      envLockfile: envLockfile({ pnpm: '11.0.0' }),
      registriesByScope: { default: 'https://mirror.example.com/' },
      rootDir: '/repo',
      storeController: {} as never,
      storeDir: '/store',
      frozenLockfile: true,
    })
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
    message: 'Cannot update packageManagerDependencies with "frozen-lockfile" because the lockfile is not up to date',
  })
})

// https://github.com/pnpm/pnpm/issues/14124: a pnpm below 11.20.0 pins
// `@pnpm/exe` beside `pnpm` for a v12 version. The entry pins the wanted
// version and cannot change which pnpm runs, so a frozen install accepts the
// block a teammate's older pnpm left behind instead of failing on it.
test('an entry for a package the running pnpm does not install from is accepted under frozenLockfile', async () => {
  resolveManifestDependencies.mockClear()

  const result = await resolvePackageManagerIntegrities('12.0.0', {
    envLockfile: envLockfile({ 'pnpm': '12.0.0', '@pnpm/exe': '12.0.0' }),
    registriesByScope: { default: 'https://mirror.example.com/' },
    rootDir: '/repo',
    storeController: {} as never,
    storeDir: '/store',
    frozenLockfile: true,
  })

  expect(result.importers['.'].packageManagerDependencies).toEqual({
    'pnpm': { specifier: '12.0.0', version: '12.0.0' },
    '@pnpm/exe': { specifier: '12.0.0', version: '12.0.0' },
  })
  expect(resolveManifestDependencies).not.toHaveBeenCalled()
})

test('an entry the lockfile carries no package for is refused under frozenLockfile', async () => {
  const withoutRecords = envLockfile({ 'pnpm': '12.0.0', '@pnpm/exe': '12.0.0' })
  withoutRecords.packages = {}
  withoutRecords.snapshots = {}

  await expect(
    resolvePackageManagerIntegrities('12.0.0', {
      envLockfile: withoutRecords,
      registriesByScope: { default: 'https://mirror.example.com/' },
      rootDir: '/repo',
      storeController: {} as never,
      storeDir: '/store',
      frozenLockfile: true,
    })
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
  })
})

test('an entry pinning another version is still refused under frozenLockfile', async () => {
  await expect(
    resolvePackageManagerIntegrities('12.0.0', {
      envLockfile: envLockfile({ 'pnpm': '12.0.0', '@pnpm/exe': '11.23.0' }),
      registriesByScope: { default: 'https://mirror.example.com/' },
      rootDir: '/repo',
      storeController: {} as never,
      storeDir: '/store',
      frozenLockfile: true,
    })
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
  })
})

// A resolution that is never saved cannot take the lockfile out of sync with
// the manifest, so `--frozen-lockfile` has nothing to refuse. This is how a
// legacy `packageManager` pin below v12 switches versions.
test('an in-memory resolution is performed under frozenLockfile', async () => {
  resolveManifestDependencies.mockResolvedValueOnce({
    lockfileVersion: '9.0',
    importers: {
      '.': {
        specifiers: { pnpm: '12.0.0' },
        dependencies: { pnpm: '12.0.0' },
      },
    },
    packages: {
      'pnpm@12.0.0': { resolution: { integrity: 'sha512-pnpm' } },
    },
  } as unknown as LockfileObject)

  const result = await resolvePackageManagerIntegrities('12.0.0', {
    envLockfile: envLockfile({ pnpm: '11.0.0' }),
    registriesByScope: { default: 'https://mirror.example.com/' },
    rootDir: '/repo',
    storeController: {} as never,
    storeDir: '/store',
    save: false,
    frozenLockfile: true,
  })

  expect(result.importers['.'].packageManagerDependencies).toEqual({
    pnpm: { specifier: '12.0.0', version: '12.0.0' },
  })
})

test('a range pin records the range it asked for, not the version it resolved to', async () => {
  resolveManifestDependencies.mockResolvedValueOnce({
    lockfileVersion: '9.0',
    importers: {
      '.': {
        specifiers: { pnpm: '^12.0.0' },
        dependencies: { pnpm: '12.0.0' },
      },
    },
    packages: {
      'pnpm@12.0.0': { resolution: { integrity: 'sha512-pnpm' } },
    },
  } as unknown as LockfileObject)

  const result = await resolvePackageManagerIntegrities('12.0.0', {
    registriesByScope: { default: 'https://mirror.example.com/' },
    rootDir: '/repo',
    storeController: {} as never,
    storeDir: '/store',
    save: false,
    specifier: '^12.0.0',
  })

  expect(resolveManifestDependencies).toHaveBeenCalledWith(
    { dependencies: { pnpm: '^12.0.0' } },
    expect.anything()
  )
  expect(result.importers['.'].packageManagerDependencies).toEqual({
    pnpm: { specifier: '^12.0.0', version: '12.0.0' },
  })
})

function envLockfile (packageManagerDependencies: Record<string, string>): EnvLockfile {
  const pinned = Object.entries(packageManagerDependencies)
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        configDependencies: {},
        packageManagerDependencies: Object.fromEntries(
          pinned.map(([name, version]) => [name, { specifier: version, version }])
        ),
      },
    },
    packages: Object.fromEntries(
      pinned.map(([name, version]) => [`${name}@${version}`, { resolution: { integrity: `sha512-${name}` } }])
    ),
    snapshots: Object.fromEntries(pinned.map(([name, version]) => [`${name}@${version}`, {}])),
  } as unknown as EnvLockfile
}
