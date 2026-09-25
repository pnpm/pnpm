import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { config } from '@pnpm/config.commands'
import { PnpmError } from '@pnpm/error'
import { tempDir } from '@pnpm/prepare'
import { readIniFileSync } from 'read-ini-file'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { type ConfigFilesData, createConfigCommandOpts, readConfigFiles, writeConfigFiles } from './utils/index.js'

test('config set registry setting using the global option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@jsr:registry': 'https://alternate-jsr.example.com/',
    },
    globalYaml: {
      storeDir: '~/store',
    },
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', 'registry', 'https://npm-registry.example.com/'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    globalRc: {
      ...initConfig.globalRc,
      registry: 'https://npm-registry.example.com/',
    },
  })
})

test('config set npm-compatible setting using the global option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@jsr:registry': 'https://alternate-jsr.example.com/',
    },
    globalYaml: {
      storeDir: '~/store',
    },
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', 'cafile', 'some-cafile'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    globalRc: {
      ...initConfig.globalRc,
      cafile: 'some-cafile',
    },
  })
})

test('config set pnpm-specific key using the global option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@jsr:registry': 'https://alternate-jsr.example.com/',
    },
    globalYaml: {
      storeDir: '~/store',
    },
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', 'fetch-retries', '1'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    globalYaml: {
      ...initConfig.globalYaml,
      fetchRetries: 1,
    },
  })
})

test('config set registries and named registries using the global option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: {
      storeDir: '~/store',
    },
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  const registries = {
    default: 'https://registry.example.com/',
    '@scope': 'https://scope.example.com/',
  }
  const namedRegistries = {
    work: 'https://work.example.com/',
  }
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    json: true,
    authConfig: {},
  }), ['set', 'registries', JSON.stringify(registries)])

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    json: true,
    authConfig: {},
  }), ['set', 'named-registries', JSON.stringify(namedRegistries)])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    globalYaml: {
      ...initConfig.globalYaml,
      registries,
      namedRegistries,
    },
  })
})

test('config set using the location=global option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@jsr:registry': 'https://alternate-jsr.example.com/',
    },
    globalYaml: {
      storeDir: '~/store',
    },
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    authConfig: {},
  }), ['set', 'fetchRetries', '1'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    globalYaml: {
      ...initConfig.globalYaml,
      fetchRetries: 1,
    },
  })
})

test('config set pnpm-specific setting using the location=project option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: {
      '@jsr:registry': 'https://alternate-jsr.example.com/',
    },
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'virtual-store-dir', '.pnpm'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localYaml: {
      ...initConfig.localYaml,
      virtualStoreDir: '.pnpm',
    },
  })
})

test('config delete with location=project, when delete the last setting from pnpm-workspace.yaml, would delete the file itself', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'virtual-store-dir', '.pnpm'])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
    virtualStoreDir: '.pnpm',
  })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['delete', 'virtual-store-dir'])

  expect(fs.existsSync(path.join(tmp, 'pnpm-workspace.yaml'))).toBeFalsy()
})

test('config set registry setting using the location=project option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  const initConfig = {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: {
      '@jsr:registry': 'https://alternate-jsr.example.com/',
    },
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'registry', 'https://npm-registry.example.com/'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localRc: {
      ...initConfig.localRc,
      registry: 'https://npm-registry.example.com/',
    },
  })
})

test('config set npm-compatible setting using the location=project option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  const initConfig = {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: {
      '@jsr:registry': 'https://alternate-jsr.example.com/',
    },
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'cafile', 'some-cafile'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localRc: {
      ...initConfig.localRc,
      cafile: 'some-cafile',
    },
  })
})

test('config set saves the setting in the right format to pnpm-workspace.yaml', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'fetch-timeout', '1000'])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
    fetchTimeout: 1000,
  })
})

test('config set registry setting in project .npmrc file', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@my-company:registry': 'https://registry.my-company.example.com/',
    },
    globalYaml: {
      allowBuilds: { foo: true, bar: true },
    },
    localRc: {
      '@local:registry': 'https://localhost:7777/',
    },
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: false,
    location: 'project',
    authConfig: {},
  }), ['set', 'registry', 'https://npm-registry.example.com/'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localRc: {
      ...initConfig.localRc,
      registry: 'https://npm-registry.example.com/',
    },
  })
})

test('config set npm-compatible setting in project .npmrc file', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@my-company:registry': 'https://registry.my-company.example.com/',
    },
    globalYaml: {
      allowBuilds: { foo: true, bar: true },
    },
    localRc: {
      '@local:registry': 'https://localhost:7777/',
    },
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: false,
    location: 'project',
    authConfig: {},
  }), ['set', 'cafile', 'some-cafile'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localRc: {
      ...initConfig.localRc,
      cafile: 'some-cafile',
    },
  })
})

test('config set pnpm-specific setting in project pnpm-workspace.yaml file', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@my-company:registry': 'https://registry.my-company.example.com/',
    },
    globalYaml: {
      allowBuilds: { foo: true, bar: true },
    },
    localRc: {
      '@local:registry': 'https://localhost:7777/',
    },
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: false,
    location: 'project',
    authConfig: {},
  }), ['set', 'fetch-retries', '1'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localYaml: {
      ...initConfig.localYaml,
      fetchRetries: 1,
    },
  })
})

test('config set key=value', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@my-company:registry': 'https://registry.my-company.example.com/',
    },
    globalYaml: {
      allowBuilds: { foo: true, bar: true },
    },
    localRc: {
      '@local:registry': 'https://localhost:7777/',
    },
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'fetch-retries=1'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localYaml: {
      ...initConfig.localYaml,
      fetchRetries: 1,
    },
  })
})

test('config set key=value, when value contains a "="', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: {
      '@my-company:registry': 'https://registry.my-company.example.com/',
    },
    globalYaml: {
      allowBuilds: { foo: true, bar: true },
    },
    localRc: {
      '@local:registry': 'https://localhost:7777/',
    },
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'lockfile-dir=foo=bar'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localYaml: {
      ...initConfig.localYaml,
      lockfileDir: 'foo=bar',
    },
  })
})

test('config set or delete throws missing params error', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set'])).rejects.toThrow(new PnpmError('CONFIG_NO_PARAMS', '`pnpm config set` requires the config key'))

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['delete'])).rejects.toThrow(new PnpmError('CONFIG_NO_PARAMS', '`pnpm config delete` requires the config key'))
})

test('config set with dot leading key', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: {
      storeDir: '~/store',
    },
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', '.fetchRetries', '1'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    globalYaml: {
      ...initConfig.globalYaml,
      fetchRetries: 1,
    },
  })
})

test('config set with subscripted key', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: {
      storeDir: '~/store',
    },
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', '["fetch-retries"]', '1'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    globalYaml: {
      ...initConfig.globalYaml,
      fetchRetries: 1,
    },
  })
})

test('config set rejects complex property path', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'auth.ini'), 'store-dir=~/store')

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', '.catalog.react', '19'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_DEEP_KEY',
  })
})

test('config set with location=project and json=true', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    authConfig: {},
  }), ['set', 'catalog', '{ "react": "19" }'])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toStrictEqual({
    catalog: {
      react: '19',
    },
  })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    authConfig: {},
  }), ['set', 'packageExtensions', JSON.stringify({
    '@babel/parser': {
      peerDependencies: {
        '@babel/types': '*',
      },
    },
    'jest-circus': {
      dependencies: {
        slash: '3',
      },
    },
  })])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toStrictEqual({
    catalog: {
      react: '19',
    },
    packageExtensions: {
      '@babel/parser': {
        peerDependencies: {
          '@babel/types': '*',
        },
      },
      'jest-circus': {
        dependencies: {
          slash: '3',
        },
      },
    },
  })
})

test('config set refuses writing workspace-specific settings to the global config', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: {
      storeDir: '~/store',
    },
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    json: true,
    authConfig: {},
  }), ['set', 'catalog', '{ "react": "19" }'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY',
    key: 'catalog',
  })

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    json: true,
    authConfig: {},
  }), ['set', 'packageExtensions', JSON.stringify({
    '@babel/parser': {
      peerDependencies: {
        '@babel/types': '*',
      },
    },
    'jest-circus': {
      dependencies: {
        slash: '3',
      },
    },
  })])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY',
    key: 'packageExtensions',
  })

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    json: true,
    authConfig: {},
  }), ['set', 'package-extensions', JSON.stringify({
    '@babel/parser': {
      peerDependencies: {
        '@babel/types': '*',
      },
    },
    'jest-circus': {
      dependencies: {
        slash: '3',
      },
    },
  })])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY',
    key: 'package-extensions',
  })
})

test('config set writes workspace-specific settings to pnpm-workspace.yaml', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: undefined,
    localYaml: {
      storeDir: '~/store',
    },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  const catalog = { react: '19' }
  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    authConfig: {},
  }), ['set', 'catalog', JSON.stringify(catalog)])
  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localYaml: {
      ...initConfig.localYaml,
      catalog,
    },
  })

  const packageExtensions = {
    '@babel/parser': {
      peerDependencies: {
        '@babel/types': '*',
      },
    },
    'jest-circus': {
      dependencies: {
        slash: '3',
      },
    },
  }
  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    authConfig: {},
  }), ['set', 'packageExtensions', JSON.stringify(packageExtensions)])
  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localYaml: {
      ...initConfig.localYaml,
      catalog,
      packageExtensions,
    },
  })
})

test('config set refuses kebab-case workspace-specific settings', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    authConfig: {},
  }), ['set', 'package-extensions', JSON.stringify({
    '@babel/parser': {
      peerDependencies: {
        '@babel/types': '*',
      },
    },
    'jest-circus': {
      dependencies: {
        slash: '3',
      },
    },
  })])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_WORKSPACE_KEY',
    key: 'package-extensions',
  })
})

test('config set registry-specific setting with --location=project should create .npmrc', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', '//registry.example.com/:_auth', 'test-auth-value'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    '//registry.example.com/:_auth': 'test-auth-value',
  })
  expect(fs.existsSync(path.join(tmp, 'pnpm-workspace.yaml'))).toBeFalsy()
})

test('config set scoped registry with --location=project should create .npmrc', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', '@myorg:registry', 'https://test-registry.example.com/'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    '@myorg:registry': 'https://test-registry.example.com/',
  })
  expect(fs.existsSync(path.join(tmp, 'pnpm-workspace.yaml'))).toBeFalsy()
})

// NOTE: this test gives false positive since <https://github.com/pnpm/pnpm/pull/10145>.
// TODO: fix this test.
test('config set when both pnpm-workspace.yaml and .npmrc exist, pnpm-workspace.yaml has priority', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')
  fs.writeFileSync(path.join(tmp, 'pnpm-workspace.yaml'), 'fetchRetries: 5')

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'fetch-timeout', '2000'])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
    fetchRetries: 5,
    fetchTimeout: 2000,
  })
  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    'store-dir': '~/store',
  })
})

// NOTE: this test gives false positive since <https://github.com/pnpm/pnpm/pull/10145>.
// TODO: fix this test.
test('config set when only pnpm-workspace.yaml exists, writes to it', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, 'pnpm-workspace.yaml'), 'fetchRetries: 5')

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'fetch-timeout', '3000'])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
    fetchRetries: 5,
    fetchTimeout: 3000,
  })
  expect(fs.existsSync(path.join(tmp, '.npmrc'))).toBeFalsy()
})

test('config set --global https-proxy writes to config.yaml, not auth.ini', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', 'https-proxy', 'http://proxy.example.com:8443'])

  const result = readConfigFiles(configDir, tmp)
  expect(result.globalYaml).toEqual({
    httpsProxy: 'http://proxy.example.com:8443',
  })
  // Must NOT write to auth.ini
  expect(result.globalRc).toBeUndefined()
})

test('config set --global httpProxy writes to config.yaml', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', 'httpProxy', 'http://proxy.example.com:8080'])

  const result = readConfigFiles(configDir, tmp)
  expect(result.globalYaml).toEqual({
    httpProxy: 'http://proxy.example.com:8080',
  })
  expect(result.globalRc).toBeUndefined()
})

test('config set --global no-proxy writes to config.yaml', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: undefined,
    localYaml: undefined,
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', 'no-proxy', 'localhost,127.0.0.1'])

  const result = readConfigFiles(configDir, tmp)
  expect(result.globalYaml).toEqual({
    noProxy: 'localhost,127.0.0.1',
  })
  expect(result.globalRc).toBeUndefined()
})

test.each([
  ['config-dir'],
  ['pnpmHomeDir'],
  ['state-dir'],
])('config set refuses %s using the location=project option', async (key) => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  const initConfig = {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: undefined,
    localYaml: { storeDir: '~/store' },
  } satisfies ConfigFilesData
  writeConfigFiles(configDir, tmp, initConfig)

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', key, '/tmp/somewhere'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING',
  })

  expect(readConfigFiles(configDir, tmp)).toEqual(initConfig)
})

test('config delete removes a skipped key that a project manifest already has', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  writeConfigFiles(configDir, tmp, {
    globalRc: undefined,
    globalYaml: undefined,
    localRc: undefined,
    localYaml: { configDir: '/tmp/somewhere', storeDir: '~/store' },
  } satisfies ConfigFilesData)

  // Deleting is how a user clears a manifest that already carries `configDir`,
  // so the write-side rejection must not block it.
  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['delete', 'config-dir'])

  expect(readConfigFiles(configDir, tmp).localYaml).toEqual({ storeDir: '~/store' })
})

test('config set --global does not send a machine-level key to the project manifest', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  // A project manifest refuses `configDir` as well, so the usual "put it in
  // pnpm-workspace.yaml" hint would send the user in a circle.
  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    authConfig: {},
  }), ['set', 'config-dir', '/tmp/somewhere'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY',
    hint: expect.not.stringContaining('pnpm-workspace.yaml'),
  })
})

test('config set does not suggest the camelCase spelling of a key the project manifest refuses', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  // 'Try "pnpmHomeDir"' would be a dead end: that spelling is refused too.
  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'pnpm-home-dir', '/tmp/somewhere'])).rejects.toMatchObject({
    hint: expect.not.stringContaining('pnpmHomeDir'),
  })
})

// The hints below send the user to these commands, so they have to work.
test.each([
  ['state-dir', 'stateDir'],
  ['global-dir', 'globalDir'],
])('config set --global %s writes to the global config file', async (key, written) => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', key, '/tmp/somewhere'])

  expect(readYamlFileSync(path.join(configDir, 'config.yaml'))).toEqual({ [written]: '/tmp/somewhere' })
})

test('config set hints where a kebab-case key belongs', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'state-dir', '/tmp/somewhere'])).rejects.toMatchObject({
    hint: 'Set it for the machine instead: pnpm config set --global state-dir',
  })
})

test('config set hints where a camelCase key belongs', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'pnpmHomeDir', '/tmp/somewhere'])).rejects.toMatchObject({
    hint: 'This is not a pnpm setting',
  })
})

// The file's spelling is what the reader named, whichever spelling the user types.
test.each([
  ['config-dir', 'configDir'],
  ['configDir', 'config-dir'],
])('config delete %s clears a file written as %s', async (typed, written) => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'), { [written]: '/tmp/somewhere', storeDir: '~/store' })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['delete', typed])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({ storeDir: '~/store' })
})

test('config delete clears a hand-written kebab-case key', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'), { 'config-dir': '/tmp/somewhere', storeDir: '~/store' })

  // The reader reports the spelling the user wrote, so deleting that spelling
  // has to be the remedy it implies.
  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['delete', 'config-dir'])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({ storeDir: '~/store' })
})

test.each([
  'pnpm-home-dir',
  'global-pkg-dir',
  'root-project-manifest-dir',
  'workspace-dir',
  'package-manager-registries',
  'user-config',
])('config delete clears %s, which is absent from the types table', async (key) => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'), { [key]: '/tmp/somewhere', storeDir: '~/store' })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['delete', key])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({ storeDir: '~/store' })
})

// Accepting these spellings routes them into the writer, so the no-op depends
// on the writer skipping a removal for a field the manifest does not have.
test.each([
  ['delete', 'pnpm-home-dir', undefined],
  ['delete', 'config-dir', undefined],
  ['delete', 'store-dir', undefined],
  // `castField` turns this into a removal as well, so it reaches the same path.
  ['set', 'store-dir', 'null'],
])('config %s %s is a no-op with no manifest present', async (subcommand, key, value) => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), value == null ? [subcommand, key] : [subcommand, key, value])).resolves.toBeUndefined()

  expect(fs.existsSync(path.join(tmp, 'pnpm-workspace.yaml'))).toBe(false)
})

// The global config file's warning names every key it does not accept, not
// only the refused settings, so a delete has to reach all of them.
test('config delete clears a key the global config file never accepted', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFileSync(path.join(configDir, 'config.yaml'), { autoInstallPeers: true, storeDir: '~/store' })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['delete', 'auto-install-peers'])

  expect(readYamlFileSync(path.join(configDir, 'config.yaml'))).toEqual({ storeDir: '~/store' })
})

// Every key a reader warns about has to be clearable, whichever validator would
// otherwise have rejected the spelling.
test.each([
  // Absent from `types`, so the workspace-key check rejects its kebab form.
  ['global', 'onlyBuiltDependencies', 'onlyBuiltDependencies'],
  // Refused by a project manifest as bookkeeping, not as a setting.
  ['project', 'packageManager', 'packageManager'],
  ['project', 'package-manager', 'packageManager'],
])('config delete --%s %s clears it', async (location, typed, written) => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  const isGlobal = location === 'global'
  const target = isGlobal ? path.join(configDir, 'config.yaml') : path.join(tmp, 'pnpm-workspace.yaml')
  writeYamlFileSync(target, { [written]: 'whatever', storeDir: '~/store' })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    ...(isGlobal ? { global: true } : { location: 'project' as const }),
    authConfig: {},
  }), ['delete', typed])

  expect(readYamlFileSync(target)).toEqual({ storeDir: '~/store' })
})

// The reader warns about a refused setting in the global config file too, so a
// delete has to reach it there.
test('config delete clears a refused setting from the global config.yaml', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  // The hand-written spelling, which the delete has to clear rather than only
  // the normalized one.
  writeYamlFileSync(path.join(configDir, 'config.yaml'), { 'config-dir': '/tmp/somewhere', storeDir: '~/store' })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['delete', 'configDir'])

  expect(readYamlFileSync(path.join(configDir, 'config.yaml'))).toEqual({ storeDir: '~/store' })
})

// `set <key> null` is a removal, so it must clear the key rather than be refused.
test('config set config-dir null clears it instead of being refused', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'), { configDir: '/tmp/somewhere', storeDir: '~/store' })

  await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', 'config-dir', 'null'])

  expect(readYamlFileSync(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({ storeDir: '~/store' })
})

// The same spellings must still be refused on the way in, and by the accurate error.
test.each(['pnpm-home-dir', 'workspace-dir'])('config set still refuses %s', async (key) => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await expect(config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    authConfig: {},
  }), ['set', key, '/tmp/somewhere'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING',
  })
})
