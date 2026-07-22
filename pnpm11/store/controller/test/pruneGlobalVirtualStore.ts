/// <reference path="../../../__typings__/index.d.ts"/>
import { promises as fs } from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import { registerProject } from '../src/storeController/projectRegistry.js'
import { pruneGlobalVirtualStore } from '../src/storeController/pruneGlobalVirtualStore.js'

describe('pruneGlobalVirtualStore', () => {
  it('prunes unreachable packages from a shared resolver context', async () => {
    const storeDir = temporaryDirectory()
    const projectDir = temporaryDirectory()
    const contextDir = path.join(storeDir, 'links', 'contexts', 'context-hash')
    const reachableSlot = path.join(contextDir, '@', 'reachable', '1.0.0', 'reachable-hash')
    const projectedSlot = path.join(contextDir, '@', 'projected', '1.0.0', 'projected-hash')
    const unreachableSlot = path.join(contextDir, '@', 'unreachable', '1.0.0', 'unreachable-hash')
    const reachablePackage = path.join(reachableSlot, 'node_modules', 'reachable')
    const projectedPackage = path.join(projectedSlot, 'node_modules', 'projected')
    const unreachablePackage = path.join(unreachableSlot, 'node_modules', 'unreachable')
    const contextModulesDir = path.join(contextDir, 'node_modules')

    await Promise.all([
      fs.mkdir(reachablePackage, { recursive: true }),
      fs.mkdir(projectedPackage, { recursive: true }),
      fs.mkdir(unreachablePackage, { recursive: true }),
      fs.mkdir(contextModulesDir, { recursive: true }),
      fs.mkdir(path.join(projectDir, 'node_modules'), { recursive: true }),
    ])
    await Promise.all([
      fs.symlink(
        reachablePackage,
        path.join(projectDir, 'node_modules', 'reachable'),
        process.platform === 'win32' ? 'junction' : 'dir'
      ),
      fs.symlink(
        projectedPackage,
        path.join(contextModulesDir, 'projected'),
        process.platform === 'win32' ? 'junction' : 'dir'
      ),
    ])
    await registerProject(storeDir, projectDir)

    await pruneGlobalVirtualStore(storeDir)

    await expect(fs.stat(reachableSlot)).resolves.toBeDefined()
    await expect(fs.stat(projectedSlot)).resolves.toBeDefined()
    await expect(fs.stat(unreachableSlot)).rejects.toMatchObject({ code: 'ENOENT' })
    await expect(fs.stat(contextModulesDir)).resolves.toBeDefined()
  })

  it('removes an empty shared resolver context', async () => {
    const storeDir = temporaryDirectory()
    const projectDir = temporaryDirectory()
    const contextDir = path.join(storeDir, 'links', 'contexts', 'context-hash')
    const unreachablePackage = path.join(
      contextDir,
      '@',
      'unreachable',
      '1.0.0',
      'unreachable-hash',
      'node_modules',
      'unreachable'
    )

    await Promise.all([
      fs.mkdir(unreachablePackage, { recursive: true }),
      fs.mkdir(path.join(contextDir, 'node_modules'), { recursive: true }),
      fs.mkdir(path.join(projectDir, 'node_modules'), { recursive: true }),
    ])
    await registerProject(storeDir, projectDir)

    await pruneGlobalVirtualStore(storeDir)

    await expect(fs.stat(contextDir)).rejects.toMatchObject({ code: 'ENOENT' })
  })
})
