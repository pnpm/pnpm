import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { symlinkDependency, symlinkDependencySync, symlinkDirectRootDependency } from '@pnpm/fs.symlink-dependency'
import { tempDir } from '@pnpm/prepare'

test('symlinkDependency refuses an alias that escapes node_modules', async () => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  await expect(
    symlinkDependency(path.join(tmp, 'dep'), destModulesDir, '@x/../../../etc')
  ).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test('symlinkDependencySync refuses an alias that escapes node_modules', () => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  expect(() => {
    symlinkDependencySync(path.join(tmp, 'dep'), destModulesDir, '../sibling')
  }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test('symlinkDirectRootDependency refuses an alias that escapes node_modules', async () => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  await expect(symlinkDirectRootDependency(path.join(tmp, 'dep'), destModulesDir, '../sibling', {
    linkedPackage: { name: 'dep', version: '1.0.0' },
    prefix: '',
  })).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})
