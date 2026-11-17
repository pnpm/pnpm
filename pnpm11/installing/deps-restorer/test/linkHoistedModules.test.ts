import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { statsLogger } from '@pnpm/core-loggers'
import { tempDir } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'

import { linkHoistedModules } from '../src/linkHoistedModules.js'

test('orphan removal counts paths present before cleanup', async () => {
  const lockfileDir = tempDir(false)
  const modulesDir = path.join(lockfileDir, 'node_modules')
  const installed = path.join(modulesDir, 'installed')
  const missing = path.join(modulesDir, '@scope/skipped')
  fs.mkdirSync(installed, { recursive: true })
  fs.mkdirSync(path.dirname(missing), { recursive: true })
  fs.writeFileSync(path.join(installed, 'package.json'), JSON.stringify({ name: 'installed', version: '1.0.0' }))
  const stats = jest.spyOn(statsLogger, 'debug')
  const opts = {
    configByUri: {},
    deferDependencyBuilds: false,
    depsStateCache: {},
    force: false,
    holdBackMissingBins: false,
    ignoreScripts: true,
    importerDirs: [lockfileDir],
    lockfileDir,
    sideEffectsCacheRead: false,
  }
  try {
    await linkHoistedModules({} as StoreController, {}, [installed, missing], { [lockfileDir]: {} }, opts)
    expect(stats).toHaveBeenLastCalledWith({ prefix: lockfileDir, removed: 1 })
    expect(fs.existsSync(installed)).toBe(false)

    await linkHoistedModules({} as StoreController, {}, [installed, missing], { [lockfileDir]: {} }, opts)
    expect(stats).toHaveBeenLastCalledWith({ prefix: lockfileDir, removed: 0 })

    const scope = path.dirname(missing)
    fs.rmdirSync(scope)
    fs.writeFileSync(scope, 'not a directory')
    await linkHoistedModules({} as StoreController, {}, [installed, missing], { [lockfileDir]: {} }, opts)
    expect(stats).toHaveBeenLastCalledWith({ prefix: lockfileDir, removed: 0 })
    expect(fs.readFileSync(scope, 'utf8')).toBe('not a directory')

    const parents = Array.from({ length: 17 }, (_, index) => path.join(modulesDir, `parent-${index}`))
    const children = parents.map((parent) => path.join(parent, 'node_modules/child'))
    for (const child of children) fs.mkdirSync(child, { recursive: true })
    await linkHoistedModules({} as StoreController, {}, [...parents, ...children], { [lockfileDir]: {} }, opts)
    expect(stats).toHaveBeenLastCalledWith({ prefix: lockfileDir, removed: parents.length + children.length })
    for (const parent of parents) expect(fs.existsSync(parent)).toBe(false)
  } finally {
    stats.mockRestore()
  }
})
