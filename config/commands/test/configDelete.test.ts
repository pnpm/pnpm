import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { config } from '@pnpm/config.commands'
import { tempDir } from '@pnpm/prepare'
import { readIniFileSync } from 'read-ini-file'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { createConfigCommandOpts } from './utils/index.js'

test('config delete on registry key not set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'auth.ini'), '@my-company:registry=https://registry.my-company.example.com/')

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['delete', 'registry'])

  expect(readIniFileSync(path.join(configDir, 'auth.ini'))).toEqual({
    '@my-company:registry': 'https://registry.my-company.example.com/',
  })
})

test('config delete on registry key set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'auth.ini'), 'registry=https://registry.my-company.example.com/')

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['delete', 'registry'])

  expect(readIniFileSync(path.join(configDir, 'auth.ini'))).toEqual({})
})

test('config delete on npm-compatible key not set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'auth.ini'), '@my-company:registry=https://registry.my-company.example.com/')

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['delete', 'cafile'])

  expect(readIniFileSync(path.join(configDir, 'auth.ini'))).toEqual({
    '@my-company:registry': 'https://registry.my-company.example.com/',
  })
})

test('config delete on npm-compatible key set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'auth.ini'), 'cafile=some-cafile')

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['delete', 'cafile'])

  // NOTE: pnpm currently does not delete empty rc files.
  // TODO: maybe we should?
  expect(readIniFileSync(path.join(configDir, 'auth.ini'))).toEqual({})
})

test('config delete on pnpm-specific key not set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFileSync(path.join(configDir, 'config.yaml'), {
    cacheDir: '~/cache',
  })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['delete', 'store-dir'])

  expect(readYamlFileSync(path.join(configDir, 'config.yaml'))).toStrictEqual({
    cacheDir: '~/cache',
  })
})

test('config delete on pnpm-specific key set', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFileSync(path.join(configDir, 'config.yaml'), {
    cacheDir: '~/cache',
  })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['delete', 'cache-dir'])

  expect(fs.readdirSync(configDir)).not.toContain('config.yaml')
})
