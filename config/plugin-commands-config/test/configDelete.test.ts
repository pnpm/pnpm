import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import { config } from '@pnpm/plugin-commands-config'
import { readIniFileSync } from 'read-ini-file'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

test('config delete on registry key not set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), '@my-company:registry=https://registry.my-company.example.com/')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['delete', 'registry'])

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    '@my-company:registry': 'https://registry.my-company.example.com/',
  })
})

test('config delete on registry key set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), 'registry=https://registry.my-company.example.com/')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['delete', 'registry'])

  expect(fs.readdirSync(configDir)).not.toContain('rc')
})

test('config delete on npm-compatible key not set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), '@my-company:registry=https://registry.my-company.example.com/')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['delete', 'cafile'])

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    '@my-company:registry': 'https://registry.my-company.example.com/',
  })
})

test('config delete on npm-compatible key set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), 'cafile=some-cafile')

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['delete', 'cafile'])

  // NOTE: pnpm currently does not delete empty rc files.
  // TODO: maybe we should?
  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({})
})

test('config delete on pnpm-specific key not set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFile(path.join(configDir, 'config.yaml'), {
    cacheDir: '~/cache',
  })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['delete', 'store-dir'])

  expect(readYamlFile(path.join(configDir, 'config.yaml'))).toStrictEqual({
    cacheDir: '~/cache',
  })
})

test('config delete on pnpm-specific key set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFile(path.join(configDir, 'config.yaml'), {
    cacheDir: '~/cache',
  })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['delete', 'cache-dir'])

  expect(fs.readdirSync(configDir)).not.toContain('config.yaml')
})
