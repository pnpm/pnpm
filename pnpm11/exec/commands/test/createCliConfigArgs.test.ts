import { describe, expect, test } from '@jest/globals'

import { createCliConfigArgs } from '../src/runDepsStatusCheck.js'

describe('createCliConfigArgs', () => {
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
})
