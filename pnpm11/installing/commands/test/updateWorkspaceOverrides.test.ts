import { expect, test } from '@jest/globals'
import { writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { ProjectId } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'

import {
  addUpdatedWorkspaceOverrideCandidates,
  pickUniqueUpdatedWorkspaceOverrides,
  pickUpdatedLockfileWorkspaceOverrides,
  pickUpdatedWorkspaceOverrides,
  shouldWriteUpdatedLockfileOverrides,
  type WorkspaceOverrideUpdateCandidates,
  type WorkspaceOverrideUpdateConflict,
} from '../src/updateWorkspaceOverrides.js'

test('pickUpdatedWorkspaceOverrides skips conflicting next specifiers', () => {
  const conflicts: WorkspaceOverrideUpdateConflict[] = []
  const updatedOverrides = pickUpdatedWorkspaceOverrides({
    foo: '^1.0.0',
  }, [
    {
      before: { dependencies: { foo: '^1.0.0' } },
      after: { dependencies: { foo: '^1.1.0' } },
    },
    {
      before: { dependencies: { foo: '^1.0.0' } },
      after: { dependencies: { foo: '^1.2.0' } },
    },
  ], { onConflict: (conflict) => conflicts.push(conflict) })

  expect(updatedOverrides).toBeUndefined()
  expect(conflicts).toStrictEqual([{
    alias: 'foo',
    specifiers: ['^1.1.0', '^1.2.0'],
  }])
})

test('pickUpdatedWorkspaceOverrides does not rewrite raw catalog overrides', () => {
  const updatedOverrides = pickUpdatedWorkspaceOverrides({
    foo: 'catalog:',
  }, [{
    before: { dependencies: { foo: '^1.0.0' } },
    after: { dependencies: { foo: '^1.1.0' } },
  }])

  expect(updatedOverrides).toBeUndefined()
})

test('pickUpdatedWorkspaceOverrides leaves non-matching direct overrides unchanged', () => {
  const updatedOverrides = pickUpdatedWorkspaceOverrides({
    foo: '^0.9.0',
  }, [{
    before: { dependencies: { foo: '^1.0.0' } },
    after: { dependencies: { foo: '^1.1.0' } },
  }])

  expect(updatedOverrides).toBeUndefined()
})

test('pickUpdatedWorkspaceOverrides treats unsafe aliases as own entries', () => {
  const updatedOverrides = pickUpdatedWorkspaceOverrides(
    stringRecord([['__proto__', '^1.0.0']]),
    [{
      before: { dependencies: stringRecord([['__proto__', '^1.0.0']]) },
      after: { dependencies: stringRecord([['__proto__', '^1.1.0']]) },
    }]
  )

  expect(updatedOverrides).toBeDefined()
  expect(Object.getPrototypeOf(updatedOverrides!)).toBeNull()
  expect(Object.prototype.hasOwnProperty.call(updatedOverrides, '__proto__')).toBe(true)
  expect(updatedOverrides!['__proto__']).toBe('^1.1.0')
})

test('pickUniqueUpdatedWorkspaceOverrides skips conflicts when merging per-project updates', () => {
  const candidates: WorkspaceOverrideUpdateCandidates = new Map()
  const conflicts: WorkspaceOverrideUpdateConflict[] = []

  addUpdatedWorkspaceOverrideCandidates(candidates, { foo: '^1.1.0' })
  addUpdatedWorkspaceOverrideCandidates(candidates, { foo: '^1.2.0' })
  addUpdatedWorkspaceOverrideCandidates(candidates, { bar: '^2.1.0' })

  const updatedOverrides = pickUniqueUpdatedWorkspaceOverrides(candidates, {
    onConflict: (conflict) => conflicts.push(conflict),
  })

  expect(Object.getPrototypeOf(updatedOverrides!)).toBeNull()
  expect(updatedOverrides).toEqual({
    bar: '^2.1.0',
  })
  expect(conflicts).toStrictEqual([{
    alias: 'foo',
    specifiers: ['^1.1.0', '^1.2.0'],
  }])
})

test('addUpdatedWorkspaceOverrideCandidates skips non-string runtime values', () => {
  const candidates: WorkspaceOverrideUpdateCandidates = new Map()

  addUpdatedWorkspaceOverrideCandidates(candidates, {
    foo: 1 as unknown as string,
  })

  expect(pickUniqueUpdatedWorkspaceOverrides(candidates)).toBeUndefined()
})

test('shouldWriteUpdatedLockfileOverrides skips installer-provided updates', () => {
  expect(shouldWriteUpdatedLockfileOverrides({
    foo: '^1.1.0',
  }, {
    foo: '^1.1.0',
  })).toBe(false)
})

test('shouldWriteUpdatedLockfileOverrides writes fallback workspace updates', () => {
  expect(shouldWriteUpdatedLockfileOverrides(undefined, {
    foo: '^1.1.0',
  })).toBe(true)
})

test('shouldWriteUpdatedLockfileOverrides skips empty fallback updates', () => {
  expect(shouldWriteUpdatedLockfileOverrides(undefined, {})).toBe(false)
})

test('pickUpdatedLockfileWorkspaceOverrides requires the updated manifest specifier to match the lockfile override', async () => {
  const lockfileDir = temporaryDirectory()
  await writeWantedLockfile(lockfileDir, {
    importers: {
      ['.' as ProjectId]: {
        specifiers: {},
      },
    },
    lockfileVersion: '9.0',
    overrides: {
      foo: '^1.2.0',
    },
  })

  const updatedOverrides = await pickUpdatedLockfileWorkspaceOverrides({
    lockfileDir,
    overrides: {
      foo: '^1.0.0',
    },
    projects: [{
      before: { dependencies: { foo: '^1.0.0' } },
      after: { dependencies: { foo: '^1.1.0' } },
    }],
  })

  expect(updatedOverrides).toBeUndefined()
})

test('pickUpdatedLockfileWorkspaceOverrides returns matching updated workspace overrides', async () => {
  const lockfileDir = temporaryDirectory()
  await writeWantedLockfile(lockfileDir, {
    importers: {
      ['.' as ProjectId]: {
        specifiers: {},
      },
    },
    lockfileVersion: '9.0',
    overrides: {
      foo: '^1.1.0',
    },
  })

  const updatedOverrides = await pickUpdatedLockfileWorkspaceOverrides({
    lockfileDir,
    overrides: {
      foo: '^1.0.0',
    },
    projects: [{
      before: { dependencies: { foo: '^1.0.0' } },
      after: { dependencies: { foo: '^1.1.0' } },
    }],
  })

  expect(Object.getPrototypeOf(updatedOverrides!)).toBeNull()
  expect(updatedOverrides).toEqual({
    foo: '^1.1.0',
  })
})

function stringRecord (entries: Array<[string, string]>): Record<string, string> {
  const record = Object.create(null) as Record<string, string>
  for (const [key, value] of entries) {
    Object.defineProperty(record, key, {
      value,
      writable: true,
      enumerable: true,
      configurable: true,
    })
  }
  return record
}
