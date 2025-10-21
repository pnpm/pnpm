import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import { config } from '@pnpm/plugin-commands-config'
import { readIniFileSync } from 'read-ini-file'
import { DEFAULT_OPTS } from './utils/index.js'

test('config delete', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), `store-dir=~/store
cache-dir=~/cache`)

  await config.handler({
    ...DEFAULT_OPTS,
    configDir,
    global: true,
  }, ['delete', 'store-dir'])

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    'cache-dir': '~/cache',
  })
})
