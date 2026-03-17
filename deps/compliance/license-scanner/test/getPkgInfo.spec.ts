import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { StoreIndex } from '@pnpm/store.index'

import { getPkgInfo } from '../lib/getPkgInfo.js'

export const DEFAULT_REGISTRIES = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
}

describe('licences', () => {
  let storeDir: string
  let storeIndex: StoreIndex

  beforeAll(() => {
    storeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-license-test-'))
    storeIndex = new StoreIndex(storeDir)
  })

  afterAll(() => {
    storeIndex.close()
    fs.rmSync(storeDir, { recursive: true, force: true })
  })

  test('getPkgInfo() should throw error when package info can not be fetched', async () => {
    await expect(
      getPkgInfo(
        {
          name: 'bogus-package',
          version: '1.0.0',
          id: 'bogus-package@1.0.0',
          depPath: 'bogus-package@1.0.0',
          snapshot: {
            resolution: {
              integrity: 'integrity-sha',
            },
          },
          registries: DEFAULT_REGISTRIES,
        },
        {
          storeDir,
          storeIndex,
          virtualStoreDir: 'virtual-store-dir',
          modulesDir: 'modules-dir',
          dir: 'workspace-dir',
          virtualStoreDirMaxLength: 120,
        }
      )
    ).rejects.toThrow(/Failed to find package index file for bogus-package@1\.0\.0 \(at .*\), please consider running 'pnpm install'/)
  })
})
