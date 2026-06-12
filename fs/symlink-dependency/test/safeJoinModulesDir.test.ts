import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { symlinkDependency, symlinkDependencySync, symlinkDirectRootDependency } from '@pnpm/fs.symlink-dependency'
import { tempDir } from '@pnpm/prepare'

const escapeAliases = [
  '@x/../../../etc',
  '../sibling',
  '',
  '.',
  // Reserved names that resolve *inside* `node_modules` but would
  // overwrite pnpm-owned layout, so the containment check alone can't
  // catch them.
  '.bin',
  '.pnpm',
  'node_modules',
]

test.each(escapeAliases)('symlinkDependency refuses alias %p', async (alias) => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  await expect(
    symlinkDependency(path.join(tmp, 'dep'), destModulesDir, alias)
  ).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test.each(escapeAliases)('symlinkDependencySync refuses alias %p', (alias) => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  expect(() => {
    symlinkDependencySync(path.join(tmp, 'dep'), destModulesDir, alias)
  }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test.each(escapeAliases)('symlinkDirectRootDependency refuses alias %p', async (alias) => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  await expect(symlinkDirectRootDependency(path.join(tmp, 'dep'), destModulesDir, alias, {
    linkedPackage: { name: 'dep', version: '1.0.0' },
    prefix: '',
  })).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

const validAliases = ['foo', '@scope/name', 'foo.bar']

test.each(validAliases)('symlinkDependency accepts valid alias %p', async (alias) => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  const dep = path.join(tmp, 'dep')
  fs.mkdirSync(dep)
  await expect(symlinkDependency(dep, destModulesDir, alias)).resolves.toBeDefined()
  expect(fs.existsSync(path.join(destModulesDir, alias))).toBe(true)
})
