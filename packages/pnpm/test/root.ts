import { promises as fs } from 'fs'
import path from 'path'
import { LAYOUT_VERSION } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare'
import { execPnpmSync } from './utils'

test('pnpm root', async () => {
  tempDir()
  await fs.writeFile('package.json', '{}', 'utf8')

  const result = execPnpmSync(['root'])

  expect(result.status).toBe(0)

  expect(result.stdout.toString()).toBe(path.resolve('node_modules') + '\n')
})

test('pnpm root -g', async () => {
  tempDir()

  const global = path.resolve('global')
  await fs.mkdir(global)

  const env = { XDG_DATA_HOME: global }

  const result = execPnpmSync(['root', '-g'], { env })

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toBe(path.join(global, `pnpm/global-packages/${LAYOUT_VERSION}/node_modules`) + '\n')
})
