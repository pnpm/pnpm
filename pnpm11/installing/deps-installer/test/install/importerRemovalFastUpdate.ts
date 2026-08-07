import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId, ProjectManifest } from '@pnpm/types'

import {
  hasChangedProjectSpecifiers,
  tryFastUpdateImporters,
} from '../../src/install/tryFastUpdateImporters.js'

test('a dependency the manifest dropped is noticed as a change', () => {
  expect(hasChangedProjectSpecifiers(lockfile(), [project({ bar: '^2.0.0' })])).toBe(true)
})

test('a dropped dependency is removed and its subtree pruned', () => {
  const subject = lockfile()

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0' })])).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.specifiers).toStrictEqual({ bar: '^2.0.0' })
  expect(importer.dependencies).toStrictEqual({ bar: '2.0.0' })
  expect(Object.keys(subject.packages!).sort()).toStrictEqual(['bar@2.0.0', 'child@3.0.0'])
})

test('dropping a dependency another package resolves as a peer falls back', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(foo@1.1.0)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(foo@1.1.0)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
    dependencies: { foo: '1.1.0' },
  }

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0', baz: '^4.0.0' })])).toBe(false)
})

function project (dependencies: Record<string, string>) {
  return {
    id: '.' as ProjectId,
    manifest: { dependencies } as ProjectManifest,
  }
}

function lockfile (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { foo: '^1.0.0', bar: '^2.0.0' },
        dependencies: { foo: '1.1.0', bar: '2.0.0' },
      },
    },
    packages: {
      ['foo@1.1.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
      ['bar@2.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-bar' },
        dependencies: { child: '3.0.0' },
      },
      ['child@3.0.0' as DepPath]: { resolution: { integrity: 'sha512-child' } },
    },
  }
}
