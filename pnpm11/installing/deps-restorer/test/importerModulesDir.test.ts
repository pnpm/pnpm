import path from 'node:path'

import { expect, test } from '@jest/globals'

import { importerModulesDir } from '../src/importerModulesDir.js'

const lockfileDir = path.resolve('ws')
const projectDir = path.join(lockfileDir, 'packages/foo')

test.each([
  [undefined, 'node_modules', 'packages/foo/node_modules'],
  ['vendor', 'vendor', 'packages/foo/vendor'],
  ['deps/vendor', 'deps/vendor', 'packages/foo/deps/vendor'],
  ['../shared/mods', '../shared/mods', 'packages/foo/mods'],
])('modulesDir %s', (modulesDir, rootModulesDir, projectModulesDir) => {
  expect(importerModulesDir(lockfileDir, { lockfileDir, modulesDir })).toBe(path.join(lockfileDir, rootModulesDir))
  expect(importerModulesDir(projectDir, { lockfileDir, modulesDir })).toBe(path.join(lockfileDir, projectModulesDir))
})

test('an absolute modulesDir gives each project a directory named after its last segment', () => {
  const modulesDir = path.resolve('elsewhere/mods')
  expect(importerModulesDir(lockfileDir, { lockfileDir, modulesDir })).toBe(modulesDir)
  expect(importerModulesDir(projectDir, { lockfileDir, modulesDir })).toBe(path.join(projectDir, 'mods'))
})
