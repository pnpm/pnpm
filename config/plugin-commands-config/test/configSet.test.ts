import fs from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { tempDir } from '@pnpm/prepare'
import { config } from '@pnpm/plugin-commands-config'
import { readIniFileSync } from 'read-ini-file'
import { writeIniFileSync } from 'write-ini-file'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

interface ConfigFilesData {
  globalRc: Record<string, unknown> | undefined
  globalYaml: Record<string, unknown> | undefined
  localRc: Record<string, unknown> | undefined
  localYaml: Record<string, unknown> | undefined
}

function readConfigFiles (globalConfigDir: string | undefined, localDir: string | undefined): ConfigFilesData {
  function tryRead<Return> (reader: () => Return): Return | undefined {
    try {
      return reader()
    } catch (error) {
      if (error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') {
        return undefined
      }
      throw error
    }
  }

  return {
    globalRc: globalConfigDir
      ? tryRead(() => readIniFileSync(path.join(globalConfigDir, 'rc')) as Record<string, unknown>)
      : undefined,
    globalYaml: globalConfigDir
      ? tryRead(() => readYamlFile(path.join(globalConfigDir, 'rc.yaml')))
      : undefined,
    localRc: localDir
      ? tryRead(() => readIniFileSync(path.join(localDir, '.npmrc')) as Record<string, unknown>)
      : undefined,
    localYaml: localDir
      ? tryRead(() => readYamlFile(path.join(localDir, 'pnpm-workspace.yaml')))
      : undefined,
  }
}

function writeConfigFiles (globalConfigDir: string | undefined, localDir: string | undefined, data: ConfigFilesData): void {
  if (globalConfigDir) {
    fs.mkdirSync(globalConfigDir, { recursive: true })

    if (data.globalRc) {
      writeIniFileSync(path.join(globalConfigDir, 'rc'), data.globalRc)
    }

    if (data.globalYaml) {
      writeYamlFile(path.join(globalConfigDir, 'rc.yaml'), data.globalYaml)
    }
  }

  if (localDir) {
    fs.mkdirSync(localDir, { recursive: true })

    if (data.localRc) {
      writeIniFileSync(path.join(localDir, '.npmrc'), data.localRc)
    }

    if (data.localYaml) {
      writeYamlFile(path.join(localDir, 'pnpm-workspace.yaml'), data.localYaml)
    }
  }
}

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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['set', 'registry', 'https://npm-registry.example.com/'])

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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['set', 'cafile', 'some-cafile'])

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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    globalYaml: {
      ...initConfig.globalYaml,
      fetchRetries: 1,
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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    rawConfig: {},
  }, ['set', 'fetchRetries', '1'])

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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'virtual-store-dir', '.pnpm'])

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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'registry', 'https://npm-registry.example.com/'])

  expect(readConfigFiles(configDir, tmp)).toEqual({
    ...initConfig,
    localRc: {
      ...initConfig.localRc,
      registry: 'https://npm-registry.example.com/',
    },
  })
})

test('config set pnpm-specific setting using the location=project option', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFile(path.join(tmp, 'pnpm-workspace.yaml'), {
    storeDir: '~/store',
  })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'fetch-retries', '1'])

  expect(readYamlFile(path.join(tmp, 'pnpm-workspace.yaml'))).toStrictEqual({
    fetchRetries: 1,
    storeDir: '~/store',
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
  }, ['set', 'lockfile-dir=foo=bar'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    'store-dir': '~/store',
    'lockfile-dir': 'foo=bar',
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

test('config set with dot leading key', async () => {
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
  }, ['set', '.fetchRetries', '1'])

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set with subscripted key', async () => {
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
  }, ['set', '["fetch-retries"]', '1'])

  expect(readIniFileSync(path.join(configDir, 'rc'))).toEqual({
    'store-dir': '~/store',
    'fetch-retries': '1',
  })
})

test('config set rejects complex property path', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), 'store-dir=~/store')

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {},
  }, ['set', '.catalog.react', '19'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_DEEP_KEY',
  })
})

test('config set with location=project and json=true', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    rawConfig: {},
  }, ['set', 'catalog', '{ "react": "19" }'])

  expect(readYamlFile(path.join(tmp, 'pnpm-workspace.yaml'))).toStrictEqual({
    catalog: {
      react: '19',
    },
  })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    rawConfig: {},
  }, ['set', 'packageExtensions', JSON.stringify({
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

  expect(readYamlFile(path.join(tmp, 'pnpm-workspace.yaml'))).toStrictEqual({
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
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(configDir, 'rc'), 'store-dir=~/store')

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    json: true,
    rawConfig: {},
  }, ['set', 'catalog', '{ "react": "19" }'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_RC_KEY',
    key: 'catalog',
  })

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    json: true,
    rawConfig: {},
  }, ['set', 'packageExtensions', JSON.stringify({
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
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_RC_KEY',
    key: 'packageExtensions',
  })

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'global',
    json: true,
    rawConfig: {},
  }, ['set', 'package-extensions', JSON.stringify({
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
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_RC_KEY',
    key: 'package-extensions',
  })
})

test('config set refuses writing workspace-specific settings to .npmrc', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })
  fs.writeFileSync(path.join(tmp, '.npmrc'), 'store-dir=~/store')

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    rawConfig: {},
  }, ['set', 'catalog', '{ "react": "19" }'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_RC_KEY',
    key: 'catalog',
  })

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    rawConfig: {},
  }, ['set', 'packageExtensions', JSON.stringify({
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
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_RC_KEY',
    key: 'packageExtensions',
  })

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    rawConfig: {},
  }, ['set', 'package-extensions', JSON.stringify({
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
    code: 'ERR_PNPM_CONFIG_SET_UNSUPPORTED_RC_KEY',
    key: 'package-extensions',
  })
})

test('config set refuses kebab-case workspace-specific settings', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await expect(config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    json: true,
    rawConfig: {},
  }, ['set', 'package-extensions', JSON.stringify({
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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', '//registry.example.com/:_auth', 'test-auth-value'])

  expect(readIniFileSync(path.join(tmp, '.npmrc'))).toEqual({
    '//registry.example.com/:_auth': 'test-auth-value',
  })
  expect(fs.existsSync(path.join(tmp, 'pnpm-workspace.yaml'))).toBeFalsy()
})

test('config set scoped registry with --location=project should create .npmrc', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  fs.mkdirSync(configDir, { recursive: true })

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', '@myorg:registry', 'https://test-registry.example.com/'])

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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'fetch-timeout', '2000'])

  expect(readYamlFile(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
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

  await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    location: 'project',
    rawConfig: {},
  }, ['set', 'fetch-timeout', '3000'])

  expect(readYamlFile(path.join(tmp, 'pnpm-workspace.yaml'))).toEqual({
    fetchRetries: 5,
    fetchTimeout: 3000,
  })
  expect(fs.existsSync(path.join(tmp, '.npmrc'))).toBeFalsy()
})
