import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId } from '@pnpm/types'

import { pickMaterializedImporterIds, pickResolvableImporterIds } from '../src/currentLockfileImporters.js'

// The current lockfile a filtered install of `packages/a` leaves behind: it
// lists every importer, but only `packages/a`'s packages.
const currentLockfile: LockfileObject = {
  lockfileVersion: '9.0',
  importers: {
    ['.' as ProjectId]: { specifiers: {} },
    ['packages/a' as ProjectId]: { specifiers: { foo: '1.0.0' }, dependencies: { foo: '1.0.0' } },
    ['packages/b' as ProjectId]: { specifiers: { bar: '1.0.0' }, dependencies: { bar: '1.0.0' } },
    ['packages/c' as ProjectId]: { specifiers: { a: 'link:../a' }, dependencies: { a: 'link:../a' } },
    ['packages/d' as ProjectId]: { specifiers: { foo: '1.0.0', bar: '1.0.0' }, dependencies: { foo: '1.0.0', bar: '1.0.0' } },
  },
  packages: {
    ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
  },
}
const importerIds = ['.', 'packages/a', 'packages/b', 'packages/c', 'packages/d']

test('pickMaterializedImporterIds() picks the importers whose packages are all installed', () => {
  expect(pickMaterializedImporterIds(currentLockfile, importerIds)).toStrictEqual(['packages/a'])
})

test('pickResolvableImporterIds() leaves out the importers whose packages are missing', () => {
  expect(pickResolvableImporterIds(currentLockfile, importerIds)).toStrictEqual(['.', 'packages/a', 'packages/c'])
})
