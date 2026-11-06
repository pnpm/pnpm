import path from 'node:path'

import { expect, test } from '@jest/globals'

import { importerModulesDir } from '../src/importerModulesDir.js'

const lockfileDir = path.resolve('ws')
const projectDir = path.join(lockfileDir, 'packages/foo')

test.each([
  [undefined, 'node_modules', 'packages/foo/node_modules'],
  ['vendor', 'vendor', 'packages/foo/vendor'],
  ['deps/vendor', 'deps/vendor', 'packages/foo/deps/vendor'],
])('modulesDir %s', (modulesDir, rootModulesDir, projectModulesDir) => {
  expect(importerModulesDir(lockfileDir, modulesDir)).toBe(path.join(lockfileDir, rootModulesDir))
  expect(importerModulesDir(projectDir, modulesDir)).toBe(path.join(lockfileDir, projectModulesDir))
})
