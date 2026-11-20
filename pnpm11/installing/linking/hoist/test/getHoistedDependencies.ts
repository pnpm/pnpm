import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getHoistedDependencies } from '@pnpm/installing.linking.hoist'
import type { DepPath, ProjectId } from '@pnpm/types'

test('hoists a direct dependency of a workspace project whose alias is an Object.prototype key', () => {
  const depPath = 'constructor@1.0.0' as DepPath
  const modulesDir = path.resolve('project/node_modules')
  const result = getHoistedDependencies({
    graph: {
      [depPath]: {
        dir: path.join(modulesDir, '.pnpm/constructor@1.0.0/node_modules/constructor'),
        children: {},
        optionalDependencies: new Set<string>(),
        hasBin: false,
        name: 'constructor',
        depPath,
      },
    },
    directDepsByImporterId: {
      ['.' as ProjectId]: new Map(),
      ['packages/foo' as ProjectId]: new Map([['constructor', depPath]]),
    },
    skipped: new Set(),
    privateHoistPattern: ['*'],
    publicHoistPattern: [],
    privateHoistedModulesDir: path.join(modulesDir, '.pnpm/node_modules'),
    publicHoistedModulesDir: modulesDir,
  })

  expect(result?.hoistedDependencies[depPath]).toStrictEqual({ constructor: 'private' })
})
