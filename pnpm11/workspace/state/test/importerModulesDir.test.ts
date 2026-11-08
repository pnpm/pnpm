import path from 'node:path'

import { expect, test } from '@jest/globals'

import { importerModulesDir, isStrictlyInside } from '../src/importerModulesDir.js'

const lockfileDir = path.resolve('ws')
const projectDir = path.join(lockfileDir, 'packages/foo')

test.each([
  [undefined, 'node_modules', 'packages/foo/node_modules'],
  ['vendor', 'vendor', 'packages/foo/vendor'],
  ['deps/vendor', 'deps/vendor', 'packages/foo/deps/vendor'],
  ['../shared', '../shared', 'packages/foo/node_modules'],
  [path.resolve('elsewhere'), '../elsewhere', 'packages/foo/node_modules'],
])('modulesDir %s', (modulesDir, rootModulesDir, projectModulesDir) => {
  expect(importerModulesDir(lockfileDir, { lockfileDir, modulesDir })).toBe(path.join(lockfileDir, rootModulesDir))
  expect(importerModulesDir(projectDir, { lockfileDir, modulesDir })).toBe(path.join(lockfileDir, projectModulesDir))
})

test.each([
  ['node_modules', true],
  ['..cache', true],
  ['', false],
  ['..', false],
  ['../sibling', false],
])('isStrictlyInside %s', (relative, expected) => {
  expect(isStrictlyInside(path.join(lockfileDir, relative), lockfileDir)).toBe(expected)
})
