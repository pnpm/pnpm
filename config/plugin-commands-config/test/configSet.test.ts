import fs from 'node:fs'
import path from 'node:path'
import { PnpmError } from '@pnpm/error'
import { tempDir } from '@pnpm/prepare'
import { config } from '@pnpm/plugin-commands-config'
import { readIniFileSync } from 'read-ini-file'
import { sync as readYamlFile } from 'read-yaml-file'

test('config set using the global option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set using the location=global option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    rawConfig: {},
  }, ['set', 'fetchRetries', '1'])

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set using the location=project option. The setting is written to pnpm-workspace.yaml, when .npmrc is not present', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'virtual-store-dir', '.pnpm'])

  expect(readYamlFile(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
    virtualStoreDir: '.pnpm',
  })
})

test('config delete using the location=project option. The setting in pnpm-workspace.yaml will be deleted, when .npmrc is not present', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'virtual-store-dir', '.pnpm'])

  expect(readYamlFile(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
    virtualStoreDir: '.pnpm',
  })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['delete', 'virtual-store-dir'])

  expect(fs.existsSync(path.join(tmp, 'pnpm-workspace.yaml'))).toBeFalsy()
})

test('config set using the location=project option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set saves the setting in the right format to pnpm-workspace.yaml', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'fetch-timeout', '1000'])

  expect(readYamlFile(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
    fetchTimeout: 1000,
  })
})

test('config set in project .npmrc file', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: false,
    location: 'project',
    rawConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set key=value', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'fetch-retries=1'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set key=value, when value contains a "="', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'foo=bar=qar'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    'store-dir': '~/store',
    foo: 'bar=qar',
  })
})

test('config set or delete throws missing params error', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set'])).rejects.toThrow(new PnpmError('CONFIG_NO_PARAMS', '`pnpm config set` requires the config key'))

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['delete'])).rejects.toThrow(new PnpmError('CONFIG_NO_PARAMS', '`pnpm config delete` requires the config key'))
})
