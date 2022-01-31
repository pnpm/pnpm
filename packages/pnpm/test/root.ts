import fs from 'fs'
import path from 'path'
import PATH_NAME from 'path-name'
import { LAYOUT_VERSION } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare'
import { execPnpmSync } from './utils'

test('pnpm root', async () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  const result = execPnpmSync(['root'])

  expect(result.status).toBe(0)

  expect(result.stdout.toString()).toBe(path.resolve('node_modules') + '\n')
})

test('pnpm root -g', async () => {
  tempDir()

  const global = path.resolve('global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  const result = execPnpmSync(['root', '-g'], { env })

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toBe(path.join(global, `pnpm/global-packages/${LAYOUT_VERSION}/node_modules`) + '\n')
})
