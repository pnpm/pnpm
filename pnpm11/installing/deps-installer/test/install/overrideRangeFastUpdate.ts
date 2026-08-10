import { expect, jest, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { RequestPackageFunction } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, Registries } from '@pnpm/types'

import { tryFastUpdateOverrides } from '../../src/install/tryFastUpdateOverrides.js'

const registries: Registries = { default: 'https://registry.npmjs.org/' }

/**
 * Two importers hold `foo` at 1.0.0 and 1.2.0. The registry also has 1.3.0,
 * which nothing locks — resolution prefers a version the graph already
 * holds, so a range override has to land on 1.2.0 rather than 1.3.0.
 */
function makeLockfile (): LockfileObject {
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '1.0.0' },
      },
      ['pkg-a' as ProjectId]: {
        dependencies: { foo: '1.2.0' },
        specifiers: { foo: '1.2.0' },
      },
    },
    packages: {
      ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-AAAA' } },
      ['foo@1.2.0' as DepPath]: { resolution: { integrity: 'sha512-BBBB' } },
    },
  } as unknown as LockfileObject
}

function opts (newValue: string, requestPackage: RequestPackageFunction) {
  return {
    lockfileDir: '/test',
    overrides: { foo: newValue },
    parsedOverrides: [{ selector: 'foo', newBareSpecifier: newValue, targetPkg: { name: 'foo' } }],
    registries,
    requestPackage,
    isLockfileUpToDate: async () => true,
  } as never
}

/** Records the version each resolution asks for, then stops the rewrite. */
function trackResolvedVersions (requested: string[]): RequestPackageFunction {
  return jest.fn(async (wanted: { bareSpecifier?: string }) => {
    requested.push(wanted.bareSpecifier!)
    throw new Error('reached the resolution step')
  }) as unknown as RequestPackageFunction
}

test('a range override resolves the version the graph already holds', async () => {
  const requested: string[] = []

  await expect(tryFastUpdateOverrides(makeLockfile(), opts('^1.1.0', trackResolvedVersions(requested))))
    .rejects.toThrow('reached the resolution step')

  expect(requested).toStrictEqual(['1.2.0'])
})

test('a range override no locked version satisfies bails before resolving', async () => {
  const requestPackage = jest.fn() as unknown as RequestPackageFunction

  const applied = await tryFastUpdateOverrides(makeLockfile(), opts('^2.0.0', requestPackage))

  expect(applied).toBe(false)
  // Only the resolver can fetch a version the lockfile does not hold.
  expect(requestPackage).not.toHaveBeenCalled()
})

test('an exact override still names the version it was given', async () => {
  const requested: string[] = []

  await expect(tryFastUpdateOverrides(makeLockfile(), opts('1.3.0', trackResolvedVersions(requested))))
    .rejects.toThrow('reached the resolution step')

  expect(requested).toStrictEqual(['1.3.0'])
})
