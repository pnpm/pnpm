import { expect, test } from '@jest/globals'
import { isPackageManagerResolved } from '@pnpm/installing.env-installer'
import type { EnvLockfile } from '@pnpm/lockfile.types'

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
