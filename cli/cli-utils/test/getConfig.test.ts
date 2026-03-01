/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import { calcPnpmfilePathsOfPluginDeps, getConfig } from '@pnpm/cli-utils'
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

describe('calcPnpmfilePathsOfPluginDeps', () => {
  test('yields pnpmfile.mjs when it exists', () => {
    const tmpDir = fs.mkdtempSync(path.join(import.meta.dirname, '.tmp-'))
    try {
      const pluginDir = path.join(tmpDir, 'pnpm-plugin-foo')
      fs.mkdirSync(pluginDir, { recursive: true })
      fs.writeFileSync(path.join(pluginDir, 'pnpmfile.mjs'), '')
      fs.writeFileSync(path.join(pluginDir, 'pnpmfile.cjs'), '')

      const paths = [...calcPnpmfilePathsOfPluginDeps(tmpDir, { 'pnpm-plugin-foo': '1.0.0' })]
      expect(paths).toEqual([path.join(pluginDir, 'pnpmfile.mjs')])
    } finally {
      fs.rmSync(tmpDir, { recursive: true })
    }
  })

  test('falls back to pnpmfile.cjs when pnpmfile.mjs does not exist', () => {
    const tmpDir = fs.mkdtempSync(path.join(import.meta.dirname, '.tmp-'))
    try {
      const pluginDir = path.join(tmpDir, 'pnpm-plugin-foo')
      fs.mkdirSync(pluginDir, { recursive: true })
      fs.writeFileSync(path.join(pluginDir, 'pnpmfile.cjs'), '')

      const paths = [...calcPnpmfilePathsOfPluginDeps(tmpDir, { 'pnpm-plugin-foo': '1.0.0' })]
      expect(paths).toEqual([path.join(pluginDir, 'pnpmfile.cjs')])
    } finally {
      fs.rmSync(tmpDir, { recursive: true })
    }
  })
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
