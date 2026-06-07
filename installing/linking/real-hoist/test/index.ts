// cspell:ignore Ffoo -- `%2F` percent-encoding of `packages/foo` in the hoisting-limits locator keys
import { expect, test } from '@jest/globals'
import { getHoistingLimits, hoist } from '@pnpm/installing.linking.real-hoist'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.utils'
import { fixtures } from '@pnpm/test-fixtures'
import type { ProjectId } from '@pnpm/types'

const f = fixtures(import.meta.dirname)

test('hoist', async () => {
  const lockfile = await readWantedLockfile(f.find('fixture'), { ignoreIncompatible: true })
  expect(hoist(lockfile!)).toBeTruthy()
})

test('hoist throws an error if the lockfile is broken', () => {
  expect(() => hoist({
    lockfileVersion: '5',
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '1.0.0',
        },
      },
    },
    packages: {},
  })).toThrow(/Broken lockfile/)
})

const importersLockfile: Pick<LockfileObject, 'importers'> = {
  importers: {
    ['.' as ProjectId]: {
      dependencies: { a: '1.0.0' },
      specifiers: { a: '1.0.0' },
    },
    ['packages/foo' as ProjectId]: {
      dependencies: { b: '1.0.0' },
      specifiers: { b: '1.0.0' },
    },
  },
}

test('getHoistingLimits returns undefined for the default "none" mode', () => {
  expect(getHoistingLimits(importersLockfile, undefined)).toBeUndefined()
  expect(getHoistingLimits(importersLockfile, 'none')).toBeUndefined()
})

test('getHoistingLimits in "workspaces" mode borders each workspace package at the root', () => {
  const limits = getHoistingLimits(importersLockfile, 'workspaces')
  // Only the root locator gets a border: its direct deps plus every
  // (URI-encoded) workspace package id. No per-importer border.
  expect([...limits!.keys()]).toStrictEqual(['.@'])
  expect(limits!.get('.@')).toStrictEqual(new Set(['a', 'packages%2Ffoo']))
})

test('getHoistingLimits in "dependencies" mode additionally borders each importer\'s direct deps', () => {
  const limits = getHoistingLimits(importersLockfile, 'dependencies')
  expect([...limits!.keys()].sort()).toStrictEqual(['.@', 'packages%2Ffoo@workspace:packages/foo'])
  expect(limits!.get('.@')).toStrictEqual(new Set(['a', 'packages%2Ffoo']))
  // Each non-root importer borders its own direct deps so their
  // transitives stay nested under them.
  expect(limits!.get('packages%2Ffoo@workspace:packages/foo')).toStrictEqual(new Set(['b']))
})
