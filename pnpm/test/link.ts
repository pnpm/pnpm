import path from 'path'
import PATH_NAME from 'path-name'
import fs from 'fs'
import { isExecutable } from '@pnpm/assert-project'
import { GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import { prepare, preparePackages } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from './utils/index.js'

const testLinkGlobal = (specifyGlobalOption: boolean) => async () => {
  prepare()
  fs.mkdirSync('cmd')
  process.chdir('cmd')
  fs.writeFileSync('package.json', JSON.stringify({ bin: { cmd: 'bin.js' } }), 'utf8')
  fs.writeFileSync('bin.js', `#!/usr/bin/env node
console.log("hello world");`, 'utf8')

  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const args = specifyGlobalOption ? ['link', '--global'] : ['link']
  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }
  await execPnpm(args, { env })

  const globalPrefix = path.join(global, `pnpm/global/${GLOBAL_LAYOUT_VERSION}`)
  expect(fs.existsSync(path.join(globalPrefix, 'node_modules/cmd'))).toBeTruthy()
  const ok = (value: any) => { // eslint-disable-line
    expect(value).toBeTruthy()
  }
  isExecutable(ok, path.join(pnpmHome, 'cmd'))
}

test('link globally the command of a package that has no name in package.json', testLinkGlobal(true))

test('link a package globally without specifying the global option', testLinkGlobal(false))

test('link a package from a workspace to the global package', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
  ])
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)
  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['link'], { env })

  const globalPrefix = path.join(global, `pnpm/global/${GLOBAL_LAYOUT_VERSION}`)
  expect(fs.existsSync(path.join(globalPrefix, 'node_modules/project-1'))).toBeTruthy()
})
