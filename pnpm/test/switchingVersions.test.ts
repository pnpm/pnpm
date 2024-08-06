import path from 'path'
import fs from 'fs'
import { prepare } from '@pnpm/prepare'
import { sync as writeJsonFile } from 'write-json-file'
import { execPnpmSync } from './utils'

test('switch to the pnpm version specified in the packageManager field of package.json, when manager-package-manager=versions is true', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  fs.writeFileSync('.npmrc', 'manage-package-manager-versions=true')
  writeJsonFile('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Version 9.3.0')
})

test('do not switch to the pnpm version specified in the packageManager field of package.json', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).not.toContain('Version 9.3.0')
})

test('do not switch to pnpm version that is specified not with a semver version', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  fs.writeFileSync('.npmrc', 'manage-package-manager-versions=true')
  writeJsonFile('package.json', {
    packageManager: 'pnpm@kevva/is-positive',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@kevva/is-positive')
})

test('do not switch to pnpm version when a range is specified', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  fs.writeFileSync('.npmrc', 'manage-package-manager-versions=true')
  writeJsonFile('package.json', {
    packageManager: 'pnpm@^9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@^9.3.0')
})
