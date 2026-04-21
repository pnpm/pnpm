import path from 'path'
import fs from 'fs'
import { prepare } from '@pnpm/prepare'
import { getToolDirPath } from '@pnpm/tools.path'
import { sync as writeJsonFile } from 'write-json-file'
import { execPnpmSync } from './utils/index.js'
import isWindows from 'is-windows'

test('switch to the pnpm version specified in the packageManager field of package.json', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Version 9.3.0')
})

test('do not switch to the pnpm version specified in the packageManager field of package.json, if manage-package-manager-versions is set to false', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  fs.writeFileSync('.npmrc', 'manage-package-manager-versions=false')
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
  writeJsonFile('package.json', {
    packageManager: 'pnpm@kevva/is-positive',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@kevva/is-positive')
})

test('do not switch to pnpm version that is specified starting with v', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@v9.15.5',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@v9.15.5: you need to specify the version as "9.15.5"')
})

test('do not switch to pnpm version when a range is specified', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@^9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@^9.3.0')
})

test('commands that v10 passes through to npm keep passing through when packageManager selects pnpm v10', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@10.0.0',
  })

  const { stdout } = execPnpmSync(['version', '--help'], { env })

  // npm's version help has this at the top — if we saw it, the argv[0]
  // passthrough fired as it always has on pnpm v10. (See #11328 for the
  // complementary v11 case, covered by readWantedPnpmMajor unit tests.)
  expect(stdout.toString()).toContain('Bump a package version')
})

test('throws error if pnpm tools dir is corrupt', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  const version = '9.3.0'
  fs.writeFileSync('.npmrc', 'manage-package-manager-versions=true')
  writeJsonFile('package.json', {
    packageManager: `pnpm@${version}`,
  })

  // Run pnpm once to ensure the tools dir is created.
  execPnpmSync(['help'], { env })

  // Intentionally corrupt the tool dir.
  const toolDir = getToolDirPath({ pnpmHomeDir: pnpmHome, tool: { name: 'pnpm', version } })
  fs.rmSync(path.join(toolDir, 'bin/pnpm'))
  if (isWindows()) {
    fs.rmSync(path.join(toolDir, 'bin/pnpm.cmd'))
  }

  const { stderr } = execPnpmSync(['help'], { env })
  expect(stderr.toString()).toContain('Failed to switch pnpm to v9.3.0. Looks like pnpm CLI is missing')
})
