import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { validateCiFrozenInstall } from '@pnpm/installing.commands'
import { prepareEmpty } from '@pnpm/prepare'

import { DEFAULT_OPTS } from './utils/index.js'

test('validateCiFrozenInstall rethrows unexpected realpath errors instead of swallowing them', async () => {
  prepareEmpty()
  const projectDir = process.cwd()
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({ name: 'app', version: '1.0.0' }))
  const blockedModulesParent = path.join(projectDir, 'blocked-parent')
  const eacces = Object.assign(new Error('EACCES: permission denied'), { code: 'EACCES' })
  const originalRealpath = fs.promises.realpath.bind(fs.promises)
  const spy = jest.spyOn(fs.promises, 'realpath').mockImplementation(async (target, options) => {
    if (String(target) === blockedModulesParent) {
      throw eacces
    }
    return originalRealpath(target, options as Parameters<typeof fs.promises.realpath>[1])
  })
  try {
    await expect(validateCiFrozenInstall({
      ...DEFAULT_OPTS,
      allProjects: [],
      dir: projectDir,
      lockfileDir: projectDir,
      modulesDir: path.join(blockedModulesParent, 'node_modules'),
    })).rejects.toThrow('EACCES: permission denied')
  } finally {
    spy.mockRestore()
  }
})
