import { config } from '@pnpm/plugin-commands-config'
import { runNpm } from '@pnpm/run-npm'

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
  const configOpts = {
    dir: process.cwd(),
    cliOptions: {},
    configDir: __dirname, // this doesn't matter, it won't be used
    rawConfig: {},
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
