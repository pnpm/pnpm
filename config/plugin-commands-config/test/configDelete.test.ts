import fs from 'node:fs'
import path from 'node:path'

import { readIniFileSync } from 'read-ini-file'

import { tempDir } from '@pnpm/prepare'
import { config } from '@pnpm/plugin-commands-config'

test('config delete', async (): Promise<void> => {
  const tmp = tempDir()

  const configDir = path.join(tmp, 'global-config')

  fs.mkdirSync(configDir, { recursive: true })

  fs.writeFileSync(
    path.join(configDir, 'rc'),
    `store-dir=~/store
cache-dir=~/cache`
  )

  await config.handler(
    {
      dir: process.cwd(),
      cliOptions: {},
      configDir,
      global: true,
      rawConfig: {},
    },
    ['delete', 'store-dir']
  )

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    'cache-dir': '~/cache',
  })
})
