/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import { getConfig } from '@pnpm/cli-utils'
import { prepare } from '@pnpm/prepare'
import { jest } from '@jest/globals'

beforeEach(() => {
  jest.spyOn(console, 'warn')
})

afterEach(() => {
  jest.mocked(console.warn).mockRestore()
})

test('console a warning when the .npmrc has an env variable that does not exist', async () => {
  prepare()

  fs.writeFileSync('.npmrc', 'registry=${ENV_VAR_123}', 'utf8')

  await getConfig({
    json: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
    rcOptionsTypes: {},
  })

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
