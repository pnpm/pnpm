import path from 'node:path'

import { expect, test } from '@jest/globals'
import { findHoistedPackageDirs, withCollapsedVariants } from '@pnpm/deps.path'

const lockfileDir = path.resolve('project')

test('findHoistedPackageDirs finds the location recorded for another peer variant', () => {
  const hoistedLocations = withCollapsedVariants({
    'foo@1.0.0(bar@1.0.0)': ['node_modules/foo/node_modules/qar'],
  })
  expect(findHoistedPackageDirs(hoistedLocations, 'foo@1.0.0(bar@2.0.0)', lockfileDir))
    .toStrictEqual([path.join(lockfileDir, 'node_modules/foo/node_modules/qar')])
})

test('findHoistedPackageDirs drops locations that leave the lockfile directory', () => {
  expect(findHoistedPackageDirs({ 'foo@1.0.0': ['../outside/foo', 'node_modules/foo'] }, 'foo@1.0.0', lockfileDir))
    .toStrictEqual([path.join(lockfileDir, 'node_modules/foo')])
})

test('findHoistedPackageDirs finds a collapsed variant recorded under a legacy key', () => {
  const hoistedLocations = withCollapsedVariants({
    '/foo@1.0.0(bar@1.0.0)': ['node_modules/foo'],
  })
  expect(findHoistedPackageDirs(hoistedLocations, 'foo@1.0.0(bar@2.0.0)', lockfileDir))
    .toStrictEqual([path.join(lockfileDir, 'node_modules/foo')])
})
