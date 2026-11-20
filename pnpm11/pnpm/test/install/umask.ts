import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'

import { execPnpm } from '../utils/index.js'

const testOnLinuxOnly = (process.platform === 'darwin' || process.platform === 'win32') ? test.skip : test

const pkgName = 'local-pkg'
const pkgVersion = '1.0.0'

// The virtual-store dir of a `file:` tarball dependency carries the specifier.
function pkgNodeModulesDir (projectRoot: string): string {
  const pnpmDir = path.join(projectRoot, 'node_modules', '.pnpm')
  const id = fs.readdirSync(pnpmDir).find(name => name.startsWith(`${pkgName}@`))
  expect(id).toBeTruthy()
  return path.join(pnpmDir, id!, 'node_modules', pkgName)
}

// The store keeps its content under a versioned directory.
function findExecStoreFiles (storeDir: string): string[] {
  const filesRoot = path.join(storeDir, STORE_VERSION, 'files')
  const execFiles: string[] = []
  for (const hexDir of fs.readdirSync(filesRoot)) {
    const hexPath = path.join(filesRoot, hexDir)
    if (!fs.statSync(hexPath).isDirectory()) continue
    for (const name of fs.readdirSync(hexPath)) {
      if (name.endsWith('-exec')) execFiles.push(path.join(hexPath, name))
    }
  }
  return execFiles
}

// A store populated under one umask is imported into each project under that
// project's own umask (pnpm/pnpm#3807).
testOnLinuxOnly('entries imported from a shared store carry the importing project\'s umask', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-umask-'))
  const sharedStore = path.join(root, 'store')
  const originalDir = process.cwd()
  const originalUmask = process.umask()
  try {
    // A bin, an executable that is not a bin, a plain module, and a manifest.
    const pkgDir = path.join(root, 'local-pkg')
    fs.mkdirSync(path.join(pkgDir, 'bin'), { recursive: true })
    fs.mkdirSync(path.join(pkgDir, 'lib'), { recursive: true })
    fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({
      name: pkgName,
      version: pkgVersion,
      bin: { [pkgName]: 'bin/cli.js' },
      files: ['bin', 'lib', 'package.json'],
    }))
    fs.writeFileSync(path.join(pkgDir, 'bin', 'cli.js'), '#!/usr/bin/env node\nexit 0\n')
    fs.chmodSync(path.join(pkgDir, 'bin', 'cli.js'), 0o755)
    fs.writeFileSync(path.join(pkgDir, 'lib', 'tool.sh'), '#!/bin/sh\nexit 0\n')
    fs.chmodSync(path.join(pkgDir, 'lib', 'tool.sh'), 0o755)
    fs.writeFileSync(path.join(pkgDir, 'lib', 'index.js'), 'module.exports = 1\n')

    process.chdir(pkgDir)
    await execPnpm(['pack'])
    const tarball = path.join(pkgDir, 'local-pkg-1.0.0.tgz')
    expect(fs.existsSync(tarball)).toBe(true)

    const install = async (projectName: string, umask: number): Promise<void> => {
      const project = path.join(root, projectName)
      fs.mkdirSync(project)
      fs.writeFileSync(path.join(project, 'package.json'), JSON.stringify({
        name: `project-${projectName}`,
        version: '1.0.0',
        dependencies: { [pkgName]: `file:${tarball}` },
      }))
      process.chdir(project)
      process.umask(umask)
      await execPnpm(['install', '--store-dir', sharedStore])
    }

    // Populate the shared store under umask 022.
    await install('a', 0o022)
    const storeExecFiles = findExecStoreFiles(sharedStore)
    expect(storeExecFiles).toHaveLength(2)
    const storeExecModes = storeExecFiles.map(file => fs.statSync(file).mode & 0o777)
    expect(storeExecModes).toEqual([0o755, 0o755])
    const dirA = pkgNodeModulesDir(path.join(root, 'a'))
    expect(fs.statSync(path.join(dirA, 'bin', 'cli.js')).mode & 0o777).toBe(0o755)
    expect(fs.statSync(path.join(dirA, 'lib', 'tool.sh')).mode & 0o777).toBe(0o755)
    expect(fs.statSync(path.join(dirA, 'lib', 'index.js')).mode & 0o777).toBe(0o644)

    // Import from it under umask 077, where the desired modes are
    // 0o755 & ~0o077 = 0o700 and 0o666 & ~0o077 = 0o600. Linking the bin then
    // adds the execute bits that are missing from its umask-restricted mode.
    await install('b', 0o077)
    const dirB = pkgNodeModulesDir(path.join(root, 'b'))
    expect(fs.statSync(path.join(dirB, 'bin', 'cli.js')).mode & 0o777).toBe(0o711)
    expect(fs.statSync(path.join(dirB, 'lib', 'tool.sh')).mode & 0o777).toBe(0o700)
    expect(fs.statSync(path.join(dirB, 'lib', 'index.js')).mode & 0o777).toBe(0o600)
    for (const file of storeExecFiles) {
      expect(fs.statSync(file).mode & 0o777).toBe(0o755)
    }
  } finally {
    process.chdir(originalDir)
    process.umask(originalUmask)
    fs.rmSync(root, { recursive: true, force: true })
  }
})
