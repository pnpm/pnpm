/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import { getConfig } from '@pnpm/cli-utils'
import { prepare } from '@pnpm/prepare'

beforeEach(() => {
  jest.spyOn(console, 'warn')
})

afterEach(() => {
  jest.mocked(console.warn).mockRestore()
})

test('console a warning when the .npmrc has an env variable that does not exist', async () => {
  prepare()

  fs.writeFileSync('.npmrc', 'foo=${ENV_VAR_123}', 'utf8') // eslint-disable-line

  await getConfig({
    json: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
    rcOptionsTypes: {},
  })

  // eslint-disable-next-line no-template-curly-in-string
  expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('Failed to replace env in config: ${ENV_VAR_123}'))
})

test('hoist: false removes hoistPattern', async () => {
  prepare()

  const config = await getConfig({
    hoist: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
    rcOptionsTypes: {},
  })

  expect(config.hoist).toBe(false)
  expect(config.hoistPattern).toBeUndefined()
})

test('allowBuilds populates onlyBuiltDependencies and ignoredBuiltDependencies', async () => {
  prepare()

  const config = await getConfig({
    allowBuilds: {
      'allowed-pkg': true,
      'another-allowed': true,
      'blocked-pkg': false,
    },
  }, {
    workspaceDir: '.',
    excludeReporter: false,
    rcOptionsTypes: {},
  })

  expect(config.onlyBuiltDependencies).toContain('allowed-pkg')
  expect(config.onlyBuiltDependencies).toContain('another-allowed')
  expect(config.ignoredBuiltDependencies).toContain('blocked-pkg')
})

test('allowBuilds does not add duplicates', async () => {
  prepare()

  const config = await getConfig({
    onlyBuiltDependencies: ['already-allowed'],
    ignoredBuiltDependencies: ['already-blocked'],
    allowBuilds: {
      'already-allowed': true,
      'already-blocked': false,
    },
  }, {
    workspaceDir: '.',
    excludeReporter: false,
    rcOptionsTypes: {},
  })

  expect(config.onlyBuiltDependencies).toEqual(['already-allowed'])
  expect(config.ignoredBuiltDependencies).toEqual(['already-blocked'])
})
