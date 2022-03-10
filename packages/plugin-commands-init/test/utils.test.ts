import { workWithInitModule, personToString } from '@pnpm/plugin-commands-init/lib/utils'
import fixtures from '@pnpm/test-fixtures'
import fs from 'fs'
import path from 'path'

const f = fixtures(path.join(__dirname, '../fixtures'))

test('run the workWithInitModule function', async () => {
  const dir = f.prepare('init-module')
  const rawConfig = {
    initVersion: '2.0.0',
    initModule: '.pnpm-init.js',
  }
  expect(workWithInitModule(rawConfig)).toEqual({
    initVersion: '2.0.0',
  })
  expect(fs.existsSync(path.resolve(dir, 'test.txt'))).toBeTruthy()
})

test('run the personToString function', () => {
  const expectAuthor = 'pnpm <xxxxxx@pnpm.com> (https://www.github.com/pnpm)'
  expect(personToString({
    email: 'xxxxxx@pnpm.com',
    name: 'pnpm',
    url: 'https://www.github.com/pnpm',
  })).toBe(expectAuthor)
})
