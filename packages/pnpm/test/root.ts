import { promises as fs } from 'fs'
import path from 'path'
import { LAYOUT_VERSION } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare'
import isWindows from 'is-windows'
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

  const env = { NPM_CONFIG_PREFIX: global }
  if (process.env.APPDATA) env['APPDATA'] = global

  const result = execPnpmSync(['root', '-g'], { env })

  expect(result.status).toBe(0)

  if (isWindows()) {
    expect(result.stdout.toString()).toBe(path.join(global, `pnpm-global/${LAYOUT_VERSION}/node_modules`) + '\n')
  } else {
    expect(result.stdout.toString()).toBe(path.join(global, `pnpm-global/${LAYOUT_VERSION}/node_modules`) + '\n')
  }
})
