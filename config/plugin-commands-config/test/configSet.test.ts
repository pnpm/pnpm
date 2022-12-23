import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import { config } from '@pnpm/plugin-commands-config'

test('install Node (and npm, npx) by exact version of Node.js', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    configDir,
    global: true,
    rawConfig: {},
    rawLocalConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(fs.readFileSync(path.join(configDir, 'rc'), 'utf8')).toBe(`store-dir=~/store
fetch-retries=1
`)
})
