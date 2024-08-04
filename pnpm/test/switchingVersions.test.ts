import path from 'path'
import fs from 'fs'
import { prepare } from '@pnpm/prepare'
import { sync as writeJsonFile } from 'write-json-file'
import { execPnpmSync } from './utils'

test('switch to the pnpm version specified in the packageManager field of package.json', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  writeJsonFile('package.json', {
    packageManager: 'pnpm@9.3.0',
  })
  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Version 9.3.0')
})
