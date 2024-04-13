import fs from 'fs'
import path from 'path'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { prepare } from '@pnpm/prepare'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { execPnpm } from './utils'

test('dlx should ignore .npmrc in the current directory', async () => {
  prepare({})
  fs.writeFileSync('.npmrc', 'hoist-pattern=', 'utf8')

  const cacheDir = path.resolve('cache')
  await execPnpm([
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${cacheDir}`,
    'dlx', 'shx', 'echo', 'hi'])

  const modulesManifest = await readModulesManifest(path.join(cacheDir, 'dlx', createBase32Hash('shx'), 'pkg/node_modules'))
  expect(modulesManifest?.hoistPattern).toStrictEqual(['*'])
})
