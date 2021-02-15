import { promises as fs } from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import PATH from 'path-name'
import { execPnpmSync } from './utils'

test('pnpm bin', async () => {
  tempDir()
  await fs.mkdir('node_modules')

  const result = execPnpmSync(['bin'])

  expect(result.status).toStrictEqual(0)
  expect(result.stdout.toString()).toBe(path.resolve('node_modules/.bin'))
})

test('pnpm bin -g', async () => {
  tempDir()

  const result = execPnpmSync(['bin', '-g'])

  expect(result.status).toStrictEqual(0)
  expect(process.env[PATH]!.includes(result.stdout.toString())).toBeTruthy()
})
