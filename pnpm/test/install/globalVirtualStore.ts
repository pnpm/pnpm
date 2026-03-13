import fs from 'node:fs'
import path from 'node:path'

import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

test('using a global virtual store', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  const storeDir = path.resolve('store')
  const globalVirtualStoreDir = path.join(storeDir, 'v11/links')
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir,
    privateHoistPattern: '*',
  })
  await execPnpm(['install'])

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
  const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
  expect(files).toHaveLength(1)
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
})
