import path from 'path'
import PATH_NAME from 'path-name'
import fs from 'fs'
import { prepare } from '@pnpm/prepare'
import { sync as writeJsonFile } from 'write-json-file'
import { execPnpmSync } from './utils'

test('global installation', async () => {
  prepare()
  process.chdir('/Volumes/src/pnpm/pnpm_tmp/108_6140/1')
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  writeJsonFile('package.json', {
    packageManager: 'pnpm@9.3.0',
  })
  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Version 9.3.0')
})
