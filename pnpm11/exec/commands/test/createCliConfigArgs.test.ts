import path from 'node:path'

import { describe, expect, test } from '@jest/globals'

import { createCliConfigArgs } from '../src/runDepsStatusCheck.js'

describe('createCliConfigArgs', () => {
  test.each([true, false])('resolves a relative store against the install root (workspace: %s)', (workspace) => {
    const dir = path.resolve('workspace/packages/project')
    const workspaceDir = workspace ? path.resolve('workspace') : undefined
    expect(createCliConfigArgs({
      dir,
      workspaceDir,
      storeDir: '.store',
      cliOptions: { 'store-dir': '.store' },
    })).toStrictEqual([
      `--config.store-dir=${path.join(workspaceDir ?? dir, '.store')}`,
    ])
  })

  test('returns empty array when rawCliConfig is not provided or empty', () => {
    expect(createCliConfigArgs({})).toStrictEqual([])
    expect(createCliConfigArgs({ rawCliConfig: {} })).toStrictEqual([])
  })

  test('converts rawCliConfig key-value pairs into --config.<key>=<value> flags', () => {
    expect(createCliConfigArgs({
      rawCliConfig: {
        'lockfile-dir': '/path/to/lockfile',
        registry: 'https://custom.registry.org/',
        frozen: true,
      },
    })).toStrictEqual([
      '--config.lockfile-dir=/path/to/lockfile',
      '--config.registry=https://custom.registry.org/',
      '--config.frozen=true',
    ])
  })

  test('supports array values', () => {
    expect(createCliConfigArgs({
      rawCliConfig: {
        'public-hoist-pattern': ['*eslint*', '*prettier*'],
      },
    })).toStrictEqual([
      '--config.public-hoist-pattern=*eslint*',
      '--config.public-hoist-pattern=*prettier*',
    ])
  })

  test('leaves out dir, which the install starts in', () => {
    expect(createCliConfigArgs({
      rawCliConfig: {
        dir: 'project',
        'lockfile-dir': '..',
      },
    })).toStrictEqual([
      '--config.lockfile-dir=..',
    ])
  })

  test('ignores null and undefined values', () => {
    expect(createCliConfigArgs({
      rawCliConfig: {
        'lockfile-dir': '/path/to/lockfile',
        foo: undefined,
        bar: null,
      },
    })).toStrictEqual([
      '--config.lockfile-dir=/path/to/lockfile',
    ])
  })

  test('forwards install-relevant cliOptions and uses resolved path overrides', () => {
    expect(createCliConfigArgs({
      cliOptions: {
        'lockfile-dir': '../rel-lockfile',
        'store-dir': '../rel-store',
        'modules-dir': 'custom_modules',
        'npmrc-auth-file': '../user.npmrc',
        offline: true,
        'frozen-lockfile': false,
        'ignore-workspace': true,
        'workspace-root': true,
        'test-pattern': ['test/**'],
        'changed-files-ignore-pattern': ['**/*.md'],
        'workspace-packages': ['pkg-a', 'pkg-b'],
        '@my-scope:registry': 'https://scope.registry.example/',
        '//scope.registry.example/:_authToken': 'secret',
        'verify-deps-before-run': 'install',
        filter: ['pkg-a'],
      },
      lockfileDir: '../rel-lockfile',
      npmrcAuthFile: '../user.npmrc',
      storeDir: '/abs/store',
      rawCliConfig: {
        'prefer-offline': true,
      },
    })).toStrictEqual([
      `--config.lockfile-dir=${path.resolve('../rel-lockfile')}`,
      `--config.store-dir=${path.resolve('/abs/store')}`,
      '--config.modules-dir=custom_modules',
      `--config.npmrc-auth-file=${path.resolve('../user.npmrc')}`,
      '--config.offline=true',
      '--config.frozen-lockfile=false',
      '--ignore-workspace',
      '--config.workspace-root=true',
      '--config.test-pattern=test/**',
      '--config.changed-files-ignore-pattern=**/*.md',
      '--workspace-packages=pkg-a',
      '--workspace-packages=pkg-b',
      '--config.@my-scope:registry=https://scope.registry.example/',
      '--config.//scope.registry.example/:_authToken=secret',
      '--config.prefer-offline=true',
    ])
  })
})
