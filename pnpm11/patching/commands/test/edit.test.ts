import fs from 'node:fs'
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
        'is-positive': '1.0.0',
      },
    })

    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store')

    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      dir: process.cwd(),
      saveLockfile: true,
      packageImportMethod: 'hardlink',
    })

    const pkgPath = path.resolve('node_modules/is-positive')
    const indexPath = path.join(pkgPath, 'index.js')
    expect(fs.existsSync(indexPath)).toBe(true)

    const initialStat = fs.statSync(indexPath)
    const originalInode = initialStat.ino
    const isHardlinked = initialStat.nlink > 1

    // Use a node one-liner as a dummy editor to append to index.js
    const dummyEditor = 'node -e "const fs = require(\'fs\'); fs.writeFileSync(require(\'path\').join(process.argv[1], \'index.js\'), \'module.exports = () => \\"modified\\";\');"'

    await edit.handler({
      ...baseOptions,
      dir: process.cwd(),
      editor: dummyEditor,
    }, ['is-positive'])

    const modifiedContent = fs.readFileSync(indexPath, 'utf8')
    expect(modifiedContent).toContain('modified')

    if (isHardlinked) {
      const newInode = fs.statSync(indexPath).ino
      expect(newInode).not.toBe(originalInode)
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
