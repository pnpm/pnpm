import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { findLockedRootNodeRuntime } from '@pnpm/lockfile.utils'
import type { DepPath, ProjectId } from '@pnpm/types'

test('findLockedRootNodeRuntime() returns the root project pin, not a dependency pin', () => {
  // A dependency's own `engines.runtime` adds a second `node@runtime:`
  // snapshot, listed before the root's pin in key order.
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { node: 'runtime:24.0.0', dep: '1.0.0' },
        dependencies: { dep: '1.0.0' },
        devDependencies: { node: 'runtime:24.0.0' },
      },
    },
    packages: {
      ['dep@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-dep' },
        dependencies: { node: 'runtime:22.0.0' },
      },
      ['node@runtime:22.0.0' as DepPath]: { resolution: { integrity: 'sha512-22' } },
      ['node@runtime:24.0.0' as DepPath]: { resolution: { integrity: 'sha512-24' } },
    },
  }
  expect(findLockedRootNodeRuntime(lockfile)).toStrictEqual({ specifier: 'runtime:24.0.0', version: '24.0.0' })

  const rootImporter = lockfile.importers['.' as ProjectId]
  delete rootImporter.devDependencies
  expect(findLockedRootNodeRuntime(lockfile)).toBeUndefined()
})
