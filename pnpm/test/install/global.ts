import path from 'path'
import PATH_NAME from 'path-name'
import fs from 'fs'
import { LAYOUT_VERSION } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import isWindows from 'is-windows'
import exists from 'path-exists'
import {
  addDistTag,
  execPnpm,
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
  fs.mkdirSync(global)

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  await execPnpm(['install', '-g', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })

  expect(await exists(path.join(global, `pnpm/global/${LAYOUT_VERSION}/node_modules/@pnpm.e2e/postinstall-calls-pnpm/created-by-postinstall`))).toBeTruthy()
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
