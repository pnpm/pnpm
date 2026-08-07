import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId, ProjectManifest } from '@pnpm/types'
import { clone } from 'ramda'

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

test('dropping a dependency another package resolves as a peer falls back without mutating the lockfile', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(foo@1.1.0)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(foo@1.1.0)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
    dependencies: { foo: '1.1.0' },
  }
  const before = clone(subject)

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0', baz: '^4.0.0' })])).toBe(false)
  expect(subject).toStrictEqual(before)
})

test('dropping a dependency together with its peer-dependent succeeds', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(foo@1.1.0)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(foo@1.1.0)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
    dependencies: { foo: '1.1.0' },
  }

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0' })])).toBe(true)
  expect(Object.keys(subject.packages!)).toStrictEqual(['bar@2.0.0', 'child@3.0.0'])
})

test('dropping a dependency when a surviving peer suffix is hashed falls back', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(sha256-abcdef)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(sha256-abcdef)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
  }

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0', baz: '^4.0.0' })])).toBe(false)
})

test('a dependency group emptied by the removal is deleted', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].devDependencies = { qux: '5.0.0' }
  subject.importers['.' as ProjectId].specifiers.qux = '^5.0.0'
  subject.packages!['qux@5.0.0' as DepPath] = { resolution: { integrity: 'sha512-qux' } }

  expect(tryFastUpdateImporters(subject, [project({ foo: '^1.0.0', bar: '^2.0.0' })])).toBe(true)
  expect(subject.importers['.' as ProjectId].devDependencies).toBeUndefined()
})

test('a catalog entry whose last referent is dropped is pruned', () => {
  const subject = lockfile()
  subject.catalogs = { default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } }
  subject.importers['.' as ProjectId].specifiers.foo = 'catalog:'

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0' })])).toBe(true)
  expect(subject.catalogs).toBeUndefined()
})

test('a catalog entry another importer references is kept', () => {
  const subject = lockfile()
  subject.catalogs = { default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } }
  subject.importers['.' as ProjectId].specifiers.foo = 'catalog:'
  subject.importers['pkg-a' as ProjectId] = {
    specifiers: { foo: 'catalog:' },
    dependencies: { foo: '1.1.0' },
  }

  expect(tryFastUpdateImporters(subject, [
    project({ bar: '^2.0.0' }),
    { id: 'pkg-a' as ProjectId, manifest: { dependencies: { foo: 'catalog:' } } as ProjectManifest },
  ])).toBe(true)
  expect(subject.catalogs).toStrictEqual({ default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } })
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
