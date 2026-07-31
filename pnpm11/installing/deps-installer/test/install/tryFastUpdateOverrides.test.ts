import { expect, jest, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { RequestPackageFunction } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, Registries } from '@pnpm/types'

import { tryFastUpdateOverrides } from '../../src/install/tryFastUpdateOverrides.js'

const registries: Registries = { default: 'https://registry.npmjs.org/' }

/** An override bumping `foo` from 1.0.0 to 2.0.0, in the shape the fast path accepts. */
const overrides = { foo: '2.0.0' }
const parsedOverrides = [
  { selector: 'foo', newBareSpecifier: '2.0.0', targetPkg: { name: 'foo' } },
] as never

function makeLockfile (depPath: string, ref: string): LockfileObject {
  return {
    lockfileVersion: '9.0',
    overrides: { foo: '1.0.0' },
    importers: {
      ['.' as ProjectId]: {
        dependencies: { foo: ref },
        specifiers: { foo: '^1.0.0' },
      },
    },
    packages: {
      [depPath as DepPath]: {
        resolution: { integrity: 'sha512-AAAA' },
      },
    },
  } as unknown as LockfileObject
}

function opts (requestPackage: RequestPackageFunction) {
  return {
    lockfileDir: '/test',
    overrides,
    parsedOverrides,
    registries,
    requestPackage,
    isLockfileUpToDate: async () => true,
  } as never
}

test('the override fast path declines a registry-qualified dependency path', async () => {
  // The fast path rebuilds keys as `<alias>@<version>`, which would turn
  // `foo@work:1.0.0` into `foo@2.0.0` and silently drop the registry the
  // package actually came from. It must bail so the caller re-resolves.
  const requestPackage = jest.fn() as unknown as RequestPackageFunction

  const applied = await tryFastUpdateOverrides(
    makeLockfile('foo@work:1.0.0', 'work:1.0.0'),
    opts(requestPackage)
  )

  expect(applied).toBe(false)
  // Bailing before any resolution is what keeps the qualifier intact.
  expect(requestPackage).not.toHaveBeenCalled()
})

test('an unqualified dependency path still reaches the resolution step', async () => {
  // Control: the same lockfile without the registry qualifier gets past the
  // key-shape guard, proving the bail above is caused by the qualifier and
  // not by unrelated fixture details.
  const requestPackage = jest.fn(async () => {
    throw new Error('stop after the guard')
  }) as unknown as RequestPackageFunction

  const applied = await tryFastUpdateOverrides(
    makeLockfile('foo@1.0.0', '1.0.0'),
    opts(requestPackage)
  ).catch(() => false)

  expect(applied).toBe(false)
  expect(requestPackage).toHaveBeenCalled()
})
