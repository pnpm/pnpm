import { expect, test } from '@jest/globals'
import type { PkgResolutionId } from '@pnpm/types'

import { addDirectDependenciesToLockfile } from '../lib/addDirectDependenciesToLockfile.js'

test('addDirectDependenciesToLockfile() locks a dependency whose alias is an Object.prototype key', () => {
  const projectSnapshot = addDirectDependenciesToLockfile({
    directDependencies: [{
      alias: 'constructor',
      dev: false,
      name: 'constructor',
      optional: false,
      pkgId: 'constructor@1.0.0' as PkgResolutionId,
      resolution: { integrity: 'sha512-AAAA', tarball: 'https://registry.npmjs.org/constructor/-/constructor-1.0.0.tgz' },
      version: '1.0.0',
    }],
    linkedPackages: [],
    newManifest: {
      dependencies: { constructor: '^1.0.0' },
    },
    projectSnapshot: { specifiers: {} },
  })
  expect(projectSnapshot).toStrictEqual({
    dependencies: { constructor: '1.0.0' },
    devDependencies: {},
    optionalDependencies: {},
    specifiers: { constructor: '^1.0.0' },
  })
})

test('addDirectDependenciesToLockfile() keeps the locked entry of an unresolved dependency whose alias is an Object.prototype key', () => {
  const projectSnapshot = addDirectDependenciesToLockfile({
    directDependencies: [],
    linkedPackages: [],
    newManifest: {
      dependencies: { constructor: '^1.0.0' },
    },
    projectSnapshot: {
      dependencies: { constructor: '1.0.0' },
      specifiers: { constructor: '^1.0.0' },
    },
  })
  expect(projectSnapshot).toStrictEqual({
    dependencies: { constructor: '1.0.0' },
    devDependencies: {},
    optionalDependencies: {},
    specifiers: { constructor: '^1.0.0' },
  })
})
