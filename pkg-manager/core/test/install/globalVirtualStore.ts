import fs from 'fs'
import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils'

test('using a global virtual store', async () => {
  prepareEmpty()
  const virtualStoreDir = path.resolve('deps')
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir,
  }))

  const files = fs.readdirSync(path.join(virtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
  expect(files.length).toBe(1)
  expect(fs.existsSync(path.join(virtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(virtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
})
