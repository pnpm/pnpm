import { workWithInitModule, personToString } from '@pnpm/plugin-commands-init/lib/utils'
import { tempDir } from '@pnpm/prepare'
import * as fs from 'fs'
import * as path from 'path'

test('run the workWithInitModule function', () => {
  const tmpDir = tempDir()
  const rawConfig = {
    initVersion: '2.0.0',
    initModule: path.join(tmpDir, 'init-module.js'),
  }
  fs.writeFileSync(rawConfig.initModule, `const fs = require('fs'); fs.writeFileSync('${path.join(tmpDir, 'test.txt')}', 'test');`)
  expect(workWithInitModule(rawConfig)).toEqual({
    initVersion: '2.0.0',
  })
  expect(fs.existsSync(path.join(tmpDir, 'test.txt'))).toBeTruthy()
})

test('run the personToString function', () => {
  const expectAuthor = 'pnpm <xxxxxx@pnpm.com> (https://www.github.com/pnpm)'
  expect(personToString({
    email: 'xxxxxx@pnpm.com',
    name: 'pnpm',
    url: 'https://www.github.com/pnpm',
  })).toBe(expectAuthor)
})
