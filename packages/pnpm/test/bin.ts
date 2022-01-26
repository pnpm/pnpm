import { promises as fs } from 'fs'
import path from 'path'
import PATH_NAME from 'path-name'
import { tempDir } from '@pnpm/prepare'
import { execPnpmSync } from './utils'

test('pnpm bin', async () => {
  tempDir()
  await fs.mkdir('node_modules')

  const result = execPnpmSync(['bin'])

  expect(result.status).toStrictEqual(0)
  expect(result.stdout.toString().trim()).toBe(path.resolve('node_modules/.bin'))
})

test('pnpm bin -g', async () => {
  tempDir()

  const env = {
    PNPM_HOME: process.cwd(),
    [PATH_NAME]: process.cwd(),
  }

  const result = execPnpmSync(['bin', '-g'], { env })

  expect(result.status).toStrictEqual(0)
  expect(result.stdout.toString().trim()).toEqual(env.PNPM_HOME)
})
