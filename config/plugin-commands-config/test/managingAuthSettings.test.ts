import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import { config } from '@pnpm/plugin-commands-config'
import { readIniFileSync } from 'read-ini-file'

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
)('auth settings are written directly to rc file', (key) => {
  describe('without --json', () => {
    it(`should set ${key} globally`, async () => {
      const tmp = tempDir()
      const configDir = path.join(tmp, 'global-config')
      fs.mkdirSync(configDir, { recursive: true })

      await config.handler({
        dir: tmp,
        cliOptions: {},
        configDir,
        global: true,
        rawConfig: {},
      }, ['set', `${key}=123`])

      const rcPath = path.join(configDir, 'rc')
      expect(readIniFileSync(rcPath)).toMatchObject({
        [key]: '123',
      })
    })
    it(`should delete ${key} globally`, async () => {
      const tmp = tempDir()
      const configDir = path.join(tmp, 'global-config')
      fs.mkdirSync(configDir, { recursive: true })
      fs.writeFileSync(path.join(configDir, 'rc'), `${key}=123\n`)

      await config.handler({
        dir: tmp,
        cliOptions: {},
        configDir,
        global: true,
        rawConfig: {},
      }, ['delete', key])

      const rcPath = path.join(configDir, 'rc')
      expect(readIniFileSync(rcPath)).not.toHaveProperty(key)
    })
  })

  describe('with --json', () => {
    it(`should set ${key} globally`, async () => {
      const tmp = tempDir()
      const configDir = path.join(tmp, 'global-config')
      fs.mkdirSync(configDir, { recursive: true })

      await config.handler({
        json: true,
        dir: tmp,
        cliOptions: {},
        configDir,
        global: true,
        rawConfig: {},
      }, ['set', key, '"123"'])

      const rcPath = path.join(configDir, 'rc')
      expect(readIniFileSync(rcPath)).toMatchObject({
        [key]: '123',
      })
    })
    it(`should delete ${key} globally`, async () => {
      const tmp = tempDir()
      const configDir = path.join(tmp, 'global-config')
      fs.mkdirSync(configDir, { recursive: true })
      fs.writeFileSync(path.join(configDir, 'rc'), `${key}=123\n`)

      await config.handler({
        json: true,
        dir: tmp,
        cliOptions: {},
        configDir,
        global: true,
        rawConfig: {},
      }, ['delete', key])

      const rcPath = path.join(configDir, 'rc')
      expect(readIniFileSync(rcPath)).not.toHaveProperty(key)
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
  const configOpts = {
    json: true,
    dir: process.cwd(),
    cliOptions: {},
    configDir: import.meta.dirname,
    rawConfig: {},
  }
  it(`${key} should reject a non-string value`, async () => {
    await expect(config.handler(configOpts, ['set', key, '{}'])).rejects.toMatchObject({
      code: 'ERR_PNPM_CONFIG_SET_AUTH_NON_STRING',
    })
  })
})

describe.each(
  [
    '._auth',
    "['_auth']",
  ]
)('%p is handled as auth setting', (propertyPath) => {
  it('should set _auth globally', async () => {
    const tmp = tempDir()
    const configDir = path.join(tmp, 'global-config')
    fs.mkdirSync(configDir, { recursive: true })

    await config.handler({
      dir: tmp,
      cliOptions: {},
      configDir,
      global: true,
      rawConfig: {},
    }, ['set', propertyPath, '123'])

    const rcPath = path.join(configDir, 'rc')
    expect(readIniFileSync(rcPath)).toMatchObject({
      _auth: '123',
    })
  })
  it('should delete _auth globally', async () => {
    const tmp = tempDir()
    const configDir = path.join(tmp, 'global-config')
    fs.mkdirSync(configDir, { recursive: true })
    fs.writeFileSync(path.join(configDir, 'rc'), '_auth=123\n')

    await config.handler({
      dir: tmp,
      cliOptions: {},
      configDir,
      global: true,
      rawConfig: {},
    }, ['delete', propertyPath])

    const rcPath = path.join(configDir, 'rc')
    expect(readIniFileSync(rcPath)).not.toHaveProperty('_auth')
  })
})
