import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.commands'
import { edit } from '@pnpm/patching.commands'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'

import { DEFAULT_OPTS } from './utils/index.js'

const baseOptions = {
  pnpmHomeDir: '',
  configByUri: {},
  registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
  userConfig: {},
  virtualStoreDir: 'node_modules/.pnpm',
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

describe('edit command', () => {
  test('edit dependency, verify de-hardlinking, edit file, and rebuild', async () => {
    prepare({
      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      },
    })

    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store')
    const projectDir = process.cwd()

    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      dir: projectDir,
      saveLockfile: true,
      packageImportMethod: 'hardlink',
    })

    const pkgPath = path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')
    const packageJsonPath = path.join(pkgPath, 'package.json')
    expect(fs.existsSync(packageJsonPath)).toBe(true)

    const initialStat = fs.statSync(packageJsonPath)
    const originalInode = initialStat.ino
    expect(initialStat.nlink).toBeGreaterThan(1)

    // The fixture has a postinstall script that creates generated-by-postinstall.js.
    // We delete it before edit to verify that rebuild runs and recreates it.
    const markerPath = path.join(pkgPath, 'generated-by-postinstall.js')
    expect(fs.existsSync(markerPath)).toBe(true)
    fs.rmSync(markerPath)

    const dummyEditor = 'node -e "const fs = require(\'fs\'); fs.writeFileSync(require(\'path\').join(process.argv[1], \'index.js\'), \'module.exports = () => \\"modified\\";\');"'

    await edit.handler({
      ...baseOptions,
      dir: projectDir,
      editor: dummyEditor,
    }, ['@pnpm.e2e/pre-and-postinstall-scripts-example'])

    const indexPath = path.join(pkgPath, 'index.js')
    const modifiedContent = fs.readFileSync(indexPath, 'utf8')
    expect(modifiedContent).toContain('modified')

    const newStat = fs.statSync(packageJsonPath)
    expect(newStat.ino).not.toBe(originalInode)
    expect(newStat.nlink).toBe(1)

    // Assert that rebuild actually ran by checking the marker file
    expect(fs.existsSync(markerPath)).toBe(true)
  })

  test('resolveSafePnpmPath excludes PATH entries under project root', async () => {
    const projectDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-edit-test-'))
    const safeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'safe-bin-'))
    try {
      const trapDir = path.join(projectDir, 'node_modules', '.bin')
      fs.mkdirSync(trapDir, { recursive: true })
      const trapPnpm = path.join(trapDir, 'pnpm')
      fs.writeFileSync(trapPnpm, '', { mode: 0o755 })

      const safePnpm = path.join(safeDir, 'pnpm')
      fs.writeFileSync(safePnpm, '', { mode: 0o755 })

      const origPath = process.env.PATH
      try {
        process.env.PATH = `${trapDir}${path.delimiter}${safeDir}`
        const editModule = await import('@pnpm/patching.commands')
        const result = editModule.edit.resolveSafePnpmPath(projectDir)
        expect(result).toBe(safePnpm)
      } finally {
        process.env.PATH = origPath
      }
    } finally {
      fs.rmSync(projectDir, { recursive: true, force: true })
      fs.rmSync(safeDir, { recursive: true, force: true })
    }
  })

  test('resolveSafePnpmPath rejects symlinked PATH entry pointing into project', async () => {
    const projectDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-edit-test-'))
    const safeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'safe-bin-'))
    const symlinkBase = fs.mkdtempSync(path.join(os.tmpdir(), 'symlink-base-'))
    try {
      const trapDir = path.join(projectDir, 'node_modules', '.bin')
      fs.mkdirSync(trapDir, { recursive: true })
      const trapPnpm = path.join(trapDir, 'pnpm')
      fs.writeFileSync(trapPnpm, '', { mode: 0o755 })

      const symlinkDir = path.join(symlinkBase, 'hijacked-bin')
      fs.symlinkSync(trapDir, symlinkDir, process.platform === 'win32' ? 'junction' : 'dir')

      const safePnpm = path.join(safeDir, 'pnpm')
      fs.writeFileSync(safePnpm, '', { mode: 0o755 })

      const origPath = process.env.PATH
      try {
        process.env.PATH = `${symlinkDir}${path.delimiter}${safeDir}`
        const editModule = await import('@pnpm/patching.commands')
        const result = editModule.edit.resolveSafePnpmPath(projectDir)
        expect(result).toBe(safePnpm)
      } finally {
        process.env.PATH = origPath
      }
    } finally {
      fs.rmSync(projectDir, { recursive: true, force: true })
      fs.rmSync(safeDir, { recursive: true, force: true })
      fs.rmSync(symlinkBase, { recursive: true, force: true })
    }
  })

  test('edit fails for missing package', async () => {
    prepare()
    const options = {
      ...baseOptions,
      dir: process.cwd(),
    }
    await expect(edit.handler(options, ['non-existent-package'])).rejects.toThrow()
  })
})
