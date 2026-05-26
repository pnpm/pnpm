import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { symlinkDependency, symlinkDependencySync, symlinkDirectRootDependency } from '@pnpm/fs.symlink-dependency'
import { tempDir } from '@pnpm/prepare'

const escapeAliases = ['@x/../../../etc', '../sibling', '', '.']

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
