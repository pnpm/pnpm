import { config } from '@pnpm/plugin-commands-config'
import { runNpm } from '@pnpm/run-npm'
import { jest } from '@jest/globals'
import { DEFAULT_OPTS } from './utils/index.js'

jest.mock('@pnpm/run-npm', () => ({
  runNpm: jest.fn(),
}))

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
)('settings related to auth are handled by npm CLI', (key) => {
  describe('without --json', () => {
    const configOpts = {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      configDir: __dirname, // this doesn't matter, it won't be used
    }
    it(`should set ${key}`, async () => {
      await config.handler(configOpts, ['set', `${key}=123`])
      expect(runNpm).toHaveBeenCalledWith(undefined, ['config', 'set', `${key}=123`])
    })
    it(`should delete ${key}`, async () => {
      await config.handler(configOpts, ['delete', key])
      expect(runNpm).toHaveBeenCalledWith(undefined, ['config', 'delete', key])
    })
  })

  describe('with --json', () => {
    const configOpts = {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      json: true,
      configDir: __dirname, // this doesn't matter, it won't be used
    }
    it(`should set ${key}`, async () => {
      await config.handler(configOpts, ['set', key, '"123"'])
      expect(runNpm).toHaveBeenCalledWith(undefined, ['config', 'set', `${key}=123`])
    })
    it(`should delete ${key}`, async () => {
      await config.handler(configOpts, ['delete', key])
      expect(runNpm).toHaveBeenCalledWith(undefined, ['config', 'delete', key])
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
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    json: true,
    configDir: __dirname, // this doesn't matter, it won't be used
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
)('%p is handled by npm CLI', (propertyPath) => {
  const configOpts = {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    configDir: __dirname, // this doesn't matter, it won't be used
  }
  it('should set _auth', async () => {
    await config.handler(configOpts, ['set', propertyPath, '123'])
    expect(runNpm).toHaveBeenCalledWith(undefined, ['config', 'set', '_auth=123'])
  })
  it('should delete _auth', async () => {
    await config.handler(configOpts, ['delete', propertyPath])
    expect(runNpm).toHaveBeenCalledWith(undefined, ['config', 'delete', '_auth'])
  })
})
