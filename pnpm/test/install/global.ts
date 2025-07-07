import path from 'path'
import PATH_NAME from 'path-name'
import fs from 'fs'
import { LAYOUT_VERSION } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import isWindows from 'is-windows'
import {
  addDistTag,
  execPnpm,
  execPnpmSync,
} from '../utils'

test('global installation', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  await execPnpm(['install', '--global', 'is-positive'], { env })

  // there was an issue when subsequent installations were removing everything installed prior
  // https://github.com/pnpm/pnpm/issues/808
  await execPnpm(['install', '--global', 'is-negative'], { env })

  const globalPrefix = path.join(global, `pnpm/global/${LAYOUT_VERSION}`)

  const { default: isPositive } = await import(path.join(globalPrefix, 'node_modules', 'is-positive'))
  expect(typeof isPositive).toBe('function')

  const { default: isNegative } = await import(path.join(globalPrefix, 'node_modules', 'is-negative'))
  expect(typeof isNegative).toBe('function')
})

test('global installation to custom directory with --global-dir', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome }

  await execPnpm(['add', '--global', '--global-dir=../global', 'is-positive'], { env })

  const { default: isPositive } = await import(path.resolve(`../global/${LAYOUT_VERSION}/node_modules/is-positive`))
  expect(typeof isPositive).toBe('function')
})

test('always install latest when doing global installation without spec', async () => {
  prepare()
  await addDistTag('@pnpm.e2e/peer-c', '2.0.0', 'latest')

  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  await execPnpm(['install', '-g', '@pnpm.e2e/peer-c@1'], { env })
  await execPnpm(['install', '-g', '@pnpm.e2e/peer-c'], { env })

  const globalPrefix = path.join(global, `pnpm/global/${LAYOUT_VERSION}`)

  process.chdir(globalPrefix)

  expect((await import(path.resolve('node_modules', '@pnpm.e2e/peer-c', 'package.json'))).default.version).toBe('2.0.0')
})

test('run lifecycle events of global packages in correct working directory', async () => {
  if (isWindows()) {
    // Skipping this test on Windows because "$npm_execpath run create-file" will fail on Windows
    return
  }

  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  const globalPkgDir = path.join(pnpmHome, 'global', String(LAYOUT_VERSION))
  fs.mkdirSync(globalPkgDir, { recursive: true })
  fs.writeFileSync(path.join(globalPkgDir, 'package.json'), JSON.stringify({ pnpm: { neverBuiltDependencies: [] } }))

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  await execPnpm(['install', '-g', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })

  expect(fs.existsSync(path.join(globalPkgDir, 'node_modules/@pnpm.e2e/postinstall-calls-pnpm/created-by-postinstall'))).toBeTruthy()
})

test('dangerously-allow-all-builds=true in global config', async () => {
  // the directory structure below applies only to Linux
  if (process.platform !== 'linux') return

  const manifest: ProjectManifest = {
    name: 'local',
    version: '0.0.0',
    private: true,
    pnpm: {
      onlyBuiltDependencies: [], // don't allow any dependencies to be built
    },
  }

  const project = prepare(manifest)

  const home = path.resolve('..', 'home/username')
  const cfgHome = path.resolve(home, '.config')
  const pnpmCfgDir = path.resolve(cfgHome, 'pnpm')
  const pnpmRcFile = path.join(pnpmCfgDir, 'rc')
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  const globalPkgDir = path.join(pnpmHome, 'global', String(LAYOUT_VERSION))
  fs.mkdirSync(pnpmCfgDir, { recursive: true })
  fs.writeFileSync(pnpmRcFile, [
    'reporter=append-only',
    'dangerously-allow-all-builds=true',
  ].join('\n'))

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    HOME: home,
    XDG_CONFIG_HOME: cfgHome,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  // global install should run scripts
  await execPnpm(['install', '-g', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.join(globalPkgDir, 'node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).toContain('created-by-postinstall')

  // local config should override global config
  await execPnpm(['add', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.resolve('node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).not.toContain('created-by-postinstall')

  // global config should be used if local config did not specify
  delete manifest.pnpm!.onlyBuiltDependencies
  project.writePackageJson(manifest)
  fs.rmSync('node_modules', { recursive: true })
  fs.rmSync('pnpm-lock.yaml')
  await execPnpm(['add', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.resolve('node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).toContain('created-by-postinstall')
})

test('dangerously-allow-all-builds=false in global config', async () => {
  // the directory structure below applies only to Linux
  if (process.platform !== 'linux') return

  const manifest: ProjectManifest = {
    name: 'local',
    version: '0.0.0',
    private: true,
    pnpm: {
      onlyBuiltDependencies: ['@pnpm.e2e/postinstall-calls-pnpm'],
    },
  }

  const project = prepare(manifest)

  const home = path.resolve('..', 'home/username')
  const cfgHome = path.resolve(home, '.config')
  const pnpmCfgDir = path.resolve(cfgHome, 'pnpm')
  const pnpmRcFile = path.join(pnpmCfgDir, 'rc')
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  const globalPkgDir = path.join(pnpmHome, 'global', String(LAYOUT_VERSION))
  fs.mkdirSync(pnpmCfgDir, { recursive: true })
  fs.writeFileSync(pnpmRcFile, [
    'reporter=append-only',
    'dangerously-allow-all-builds=false',
  ].join('\n'))

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    HOME: home,
    XDG_CONFIG_HOME: cfgHome,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  // global install should run scripts
  await execPnpm(['install', '-g', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.join(globalPkgDir, 'node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).not.toContain('created-by-postinstall')

  // local config should override global config
  await execPnpm(['add', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.resolve('node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).toContain('created-by-postinstall')

  // global config should be used if local config did not specify
  delete manifest.pnpm!.onlyBuiltDependencies
  project.writePackageJson(manifest)
  fs.rmSync('node_modules', { recursive: true })
  fs.rmSync('pnpm-lock.yaml')
  await execPnpm(['add', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.resolve('node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).not.toContain('created-by-postinstall')
})

test('global update to latest', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  await execPnpm(['install', '--global', 'is-positive@1'], { env })
  await execPnpm(['update', '--global', '--latest'], { env })

  const globalPrefix = path.join(global, `pnpm/global/${LAYOUT_VERSION}`)

  const { default: isPositive } = await import(path.join(globalPrefix, 'node_modules/is-positive/package.json'))
  expect(isPositive.version).toBe('3.1.0')
})

test('global update should not crash if there are no global packages', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  expect(execPnpmSync(['update', '--global'], { env }).status).toBe(0)
})
