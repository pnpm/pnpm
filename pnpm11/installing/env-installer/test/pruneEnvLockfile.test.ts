import { expect, test } from '@jest/globals'
import { pruneEnvLockfile, retainConfigDependencies } from '@pnpm/installing.env-installer'
import { createEnvLockfile } from '@pnpm/lockfile.fs'

test('keeps distinct config and package-manager pins of the same package', () => {
  const env = createEnvLockfile()
  env.importers['.'].configDependencies = { foo: { specifier: '1.0.0', version: '1.0.0' } }
  env.importers['.'].packageManagerDependencies = { foo: { specifier: '2.0.0', version: '2.0.0' } }
  env.packages = {
    'foo@1.0.0': { resolution: { integrity: 'sha512-config' } },
    'foo@2.0.0': { resolution: { integrity: 'sha512-manager' } },
    'shared@1.0.0': { resolution: { integrity: 'sha512-shared' } },
    'unused@1.0.0': { resolution: { integrity: 'sha512-unused' } },
  }
  env.snapshots = {
    'foo@1.0.0': { optionalDependencies: { shared: '1.0.0' } },
    'foo@2.0.0': { dependencies: { shared: '1.0.0' } },
    'shared@1.0.0': {},
    'unused@1.0.0': {},
  }

  expect(retainConfigDependencies(env, () => true)).toBe(false)
  expect(env.packages['unused@1.0.0']).toBeDefined()
  expect(env.snapshots['unused@1.0.0']).toBeDefined()
  pruneEnvLockfile(env)
  expect(Object.keys(env.packages).sort()).toEqual(['foo@1.0.0', 'foo@2.0.0', 'shared@1.0.0'])
  expect(Object.keys(env.snapshots).sort()).toEqual(['foo@1.0.0', 'foo@2.0.0', 'shared@1.0.0'])

  expect(retainConfigDependencies(env, () => false)).toBe(true)
  pruneEnvLockfile(env)
  expect(Object.keys(env.packages).sort()).toEqual(['foo@2.0.0', 'shared@1.0.0'])
  expect(Object.keys(env.snapshots).sort()).toEqual(['foo@2.0.0', 'shared@1.0.0'])
})
