import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import { config } from '@pnpm/plugin-commands-config'
import { readIniFileSync } from 'read-ini-file'

test('config set using the global option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
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
    configDir,
    location: 'global',
    rawConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set using the location=project option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set in project .npmrc file', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await config.handler({
    dir: process.cwd(),
    configDir,
    global: false,
    rawConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})
