import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { config } from '@pnpm/config.commands'
import { tempDir } from '@pnpm/prepare'

import { createConfigCommandOpts } from './utils/index.js'
import { type ConfigFilesData, readConfigFiles, writeConfigFiles } from './utils/index.js'

describe.each(
  [
    '_auth',
    '_authToken',
    '_password',
    'username',
    'registry',
    '//registry.npmjs.org/:_authToken',
  ]
)('auth settings are written to the rc file directly', (key) => {
  describe('global (without --json)', () => {
    it(`should set ${key}`, async () => {
      const tmp = tempDir()
      const configDir = path.join(tmp, 'global-config')
      const initConfig = {
        globalRc: {},
        globalYaml: undefined,
        localRc: undefined,
        localYaml: undefined,
      } satisfies ConfigFilesData
      writeConfigFiles(configDir, tmp, initConfig)

      await config.handler(createConfigCommandOpts({
        dir: tmp,
        cliOptions: {},
        configDir,
        global: true,
        authConfig: {},
      }), ['set', `${key}=123`])

      expect(readConfigFiles(configDir, tmp)).toEqual({
        ...initConfig,
        globalRc: { [key]: '123' },
      })
    })
    it(`should delete ${key}`, async () => {
      const tmp = tempDir()
      const configDir = path.join(tmp, 'global-config')
      const initConfig = {
        globalRc: { [key]: 'some-value' },
        globalYaml: undefined,
        localRc: undefined,
        localYaml: undefined,
      } satisfies ConfigFilesData
      writeConfigFiles(configDir, tmp, initConfig)

      await config.handler(createConfigCommandOpts({
        dir: tmp,
        cliOptions: {},
        configDir,
        global: true,
        authConfig: {},
      }), ['delete', key])

      expect(readConfigFiles(configDir, tmp)).toEqual({
        ...initConfig,
        globalRc: {},
      })
    })
  })

  describe('global (with --json)', () => {
    it(`should set ${key}`, async () => {
      const tmp = tempDir()
      const configDir = path.join(tmp, 'global-config')
      const initConfig = {
        globalRc: {},
        globalYaml: undefined,
        localRc: undefined,
        localYaml: undefined,
      } satisfies ConfigFilesData
      writeConfigFiles(configDir, tmp, initConfig)

      await config.handler(createConfigCommandOpts({
        json: true,
        dir: tmp,
        cliOptions: {},
        configDir,
        global: true,
        authConfig: {},
      }), ['set', key, '"123"'])

      expect(readConfigFiles(configDir, tmp)).toEqual({
        ...initConfig,
        globalRc: { [key]: '123' },
      })
    })
    it(`should delete ${key}`, async () => {
      const tmp = tempDir()
      const configDir = path.join(tmp, 'global-config')
      const initConfig = {
        globalRc: { [key]: 'some-value' },
        globalYaml: undefined,
        localRc: undefined,
        localYaml: undefined,
      } satisfies ConfigFilesData
      writeConfigFiles(configDir, tmp, initConfig)

      await config.handler(createConfigCommandOpts({
        json: true,
        dir: tmp,
        cliOptions: {},
        configDir,
        global: true,
        authConfig: {},
      }), ['delete', key])

      expect(readConfigFiles(configDir, tmp)).toEqual({
        ...initConfig,
        globalRc: {},
      })
    })
  })
})

describe.each(
  [
    '@foo:registry',
  ]
)('scoped auth settings are written to the rc file directly', (key) => {
  it(`should set ${key} globally`, async () => {
    const tmp = tempDir()
    const configDir = path.join(tmp, 'global-config')
    const initConfig = {
      globalRc: {},
      globalYaml: undefined,
      localRc: undefined,
      localYaml: undefined,
    } satisfies ConfigFilesData
    writeConfigFiles(configDir, tmp, initConfig)

    await config.handler(createConfigCommandOpts({
      dir: tmp,
      cliOptions: {},
      configDir,
      global: true,
      authConfig: {},
    }), ['set', `${key}=https://registry.example.com/`])

    expect(readConfigFiles(configDir, tmp)).toEqual({
      ...initConfig,
      globalRc: { [key]: 'https://registry.example.com/' },
    })
  })
  it(`should delete ${key} globally`, async () => {
    const tmp = tempDir()
    const configDir = path.join(tmp, 'global-config')
    const initConfig = {
      globalRc: { [key]: 'https://registry.example.com/' },
      globalYaml: undefined,
      localRc: undefined,
      localYaml: undefined,
    } satisfies ConfigFilesData
    writeConfigFiles(configDir, tmp, initConfig)

    await config.handler(createConfigCommandOpts({
      dir: tmp,
      cliOptions: {},
      configDir,
      global: true,
      authConfig: {},
    }), ['delete', key])

    expect(readConfigFiles(configDir, tmp)).toEqual({
      ...initConfig,
      globalRc: {},
    })
  })
})

describe.each(
  [
    '_auth',
    '_authToken',
    '_password',
    'username',
    'registry',
    '@foo:registry',
    '//registry.npmjs.org/:_authToken',
  ]
)('non-string values should be rejected', (key) => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')
  it(`${key} should reject a non-string value`, async () => {
    await expect(config.handler(createConfigCommandOpts({
      json: true,
      dir: tmp,
      cliOptions: {},
      configDir,
      global: true,
      authConfig: {},
    }), ['set', key, '{}'])).rejects.toMatchObject({
      code: 'ERR_PNPM_CONFIG_SET_AUTH_NON_STRING',
    })
  })
})

describe.each(
  [
    '._auth',
    "['_auth']",
  ]
)('%p is handled as an auth setting', (propertyPath) => {
  it('should set _auth', async () => {
    const tmp = tempDir()
    const configDir = path.join(tmp, 'global-config')
    const initConfig = {
      globalRc: {},
      globalYaml: undefined,
      localRc: undefined,
      localYaml: undefined,
    } satisfies ConfigFilesData
    writeConfigFiles(configDir, tmp, initConfig)

    await config.handler(createConfigCommandOpts({
      dir: tmp,
      cliOptions: {},
      configDir,
      global: true,
      authConfig: {},
    }), ['set', propertyPath, '123'])

    expect(readConfigFiles(configDir, tmp)).toEqual({
      ...initConfig,
      globalRc: { _auth: '123' },
    })
  })
  it('should delete _auth', async () => {
    const tmp = tempDir()
    const configDir = path.join(tmp, 'global-config')
    const initConfig = {
      globalRc: { _auth: 'some-value' },
      globalYaml: undefined,
      localRc: undefined,
      localYaml: undefined,
    } satisfies ConfigFilesData
    writeConfigFiles(configDir, tmp, initConfig)

    await config.handler(createConfigCommandOpts({
      dir: tmp,
      cliOptions: {},
      configDir,
      global: true,
      authConfig: {},
    }), ['delete', propertyPath])

    expect(readConfigFiles(configDir, tmp)).toEqual({
      ...initConfig,
      globalRc: {},
    })
  })
})
