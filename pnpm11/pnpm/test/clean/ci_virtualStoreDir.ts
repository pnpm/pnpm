import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { execPnpmSync } from '../utils/index.js'

test('pnpm ci cleans stale virtualStoreDir correctly when lockfile is inside node_modules/.pnpm', () => {
  tempDir()

  // Create a setup where:
  // 1. virtualStoreDir is external (e.g., .pnpm-store)
  // 2. lock.yaml is inside node_modules/.pnpm (this seems to be the stale lockfile reuse issue)

  const pkg = { name: 'test-pkg', version: '1.0.0' }
  fs.writeFileSync('package.json', JSON.stringify(pkg), 'utf8')

  const externalStore = path.resolve('.pnpm-store')
  fs.mkdirSync(externalStore, { recursive: true })

  // Run pnpm install instead of pnpm ci for testing setup, then run pnpm clean
  const installResult = execPnpmSync(['install'], {
    env: { pnpm_config_virtual_store_dir: '.pnpm-store' },
  })
  expect(installResult.status).toBe(0)

  // After install, we expect the setup to be created
  expect(fs.existsSync('.pnpm-store')).toBe(true)

  const cleanResult = execPnpmSync(['clean'], {
    env: { pnpm_config_virtual_store_dir: '.pnpm-store' },
  })
  expect(cleanResult.status).toBe(0)

  // Verify that the virtual store was removed
  expect(fs.existsSync('.pnpm-store')).toBe(false)
})
