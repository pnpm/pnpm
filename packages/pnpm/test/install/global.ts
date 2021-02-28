import path from 'path'
import { LAYOUT_VERSION } from '@pnpm/constants'
import prepare from '@pnpm/prepare'
import { promises as fs } from 'fs'
import isWindows from 'is-windows'
import exists from 'path-exists'
import {
  addDistTag,
  execPnpm,
} from '../utils'

test('global installation', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  await fs.mkdir(global)

  const env = { NPM_CONFIG_PREFIX: global }
  if (process.env.APPDATA) env['APPDATA'] = global

  await execPnpm(['install', '--global', 'is-positive'], { env })

  // there was an issue when subsequent installations were removing everything installed prior
  // https://github.com/pnpm/pnpm/issues/808
  await execPnpm(['install', '--global', 'is-negative'], { env })

  const globalPrefix = path.join(global, `pnpm-global/${LAYOUT_VERSION}`)

  const { default: isPositive } = await import(path.join(globalPrefix, 'node_modules', 'is-positive'))
  expect(typeof isPositive).toBe('function')

  const { default: isNegative } = await import(path.join(globalPrefix, 'node_modules', 'is-negative'))
  expect(typeof isNegative).toBe('function')
})

test('global installation to custom directory with --global-dir', async () => {
  prepare()

  await execPnpm(['add', '--global', '--global-dir=../global', 'is-positive'])

  const { default: isPositive } = await import(path.resolve(`../global/${LAYOUT_VERSION}/node_modules/is-positive`))
  expect(typeof isPositive).toBe('function')
})

test('always install latest when doing global installation without spec', async () => {
  prepare()
  await addDistTag('peer-c', '2.0.0', 'latest')

  const global = path.resolve('..', 'global')
  await fs.mkdir(global)

  const env = { NPM_CONFIG_PREFIX: global }

  if (process.env.APPDATA) env['APPDATA'] = global

  await execPnpm(['install', '-g', 'peer-c@1'], { env })
  await execPnpm(['install', '-g', 'peer-c'], { env })

  const globalPrefix = path.join(global, `pnpm-global/${LAYOUT_VERSION}`)

  process.chdir(globalPrefix)

  // eslint-disable-next-line @typescript-eslint/no-var-requires
  expect((await import(path.resolve('node_modules', 'peer-c', 'package.json'))).default.version).toBe('2.0.0')
})

test('run lifecycle events of global packages in correct working directory', async () => {
  if (isWindows()) {
    // Skipping this test on Windows because "$npm_execpath run create-file" will fail on Windows
    return
  }

  prepare()
  const global = path.resolve('..', 'global')
  await fs.mkdir(global)

  const env = { NPM_CONFIG_PREFIX: global }
  if (process.env.APPDATA) env['APPDATA'] = global

  await execPnpm(['install', '-g', 'postinstall-calls-pnpm@1.0.0'], { env })

  expect(await exists(path.join(global, `pnpm-global/${LAYOUT_VERSION}/node_modules/postinstall-calls-pnpm/created-by-postinstall`))).toBeTruthy()
})
