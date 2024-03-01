import path from 'path'
import PATH_NAME from 'path-name'
import fs from 'fs'
import { isExecutable } from '@pnpm/assert-project'
import { LAYOUT_VERSION } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { execPnpm } from './utils'

test('link globally the command of a package that has no name in package.json', async () => {
  prepare()
  fs.mkdirSync('cmd')
  process.chdir('cmd')
  fs.writeFileSync('package.json', JSON.stringify({ bin: { cmd: 'bin.js' } }), 'utf8')
  fs.writeFileSync('bin.js', `#!/usr/bin/env node
console.log("hello world");`, 'utf8')

  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  await execPnpm(['link', '--global'], { env })

  const globalPrefix = path.join(global, `pnpm/global/${LAYOUT_VERSION}`)
  expect(fs.existsSync(path.join(globalPrefix, 'node_modules/cmd'))).toBeTruthy()
  const ok = (value: any) => { // eslint-disable-line
    expect(value).toBeTruthy()
  }
  isExecutable(ok, path.join(pnpmHome, 'cmd'))
})
