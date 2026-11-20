import { expect, test } from '@jest/globals'
import { load } from 'js-yaml'

import { lockfileYamlDump } from '../src/lockfileYamlDump.js'
import { loadLockfileYaml } from '../src/yamlLoad.js'

test('keeps inline fields and block resolutions under explicit package keys', () => {
  const longKey = 'x'.repeat(1025)
  const lockfile = {
    packages: {
      ordinary: { resolution: { integrity: 'sha512-example' } },
      [longKey]: {
        resolution: {
          type: 'variations',
          variations: [{ os: ['linux'], note: 'first\nsecond' }],
        },
        engines: { npm: 'first\nsecond' },
      },
    },
  }

  const serialized = lockfileYamlDump(lockfile)

  expect(serialized).toBe([
    'packages:',
    '',
    '  ordinary:',
    '    resolution: {integrity: sha512-example}',
    '',
    `  ? ${longKey}`,
    '  : resolution:',
    '      type: variations',
    '      variations:',
    '        - os: [linux]',
    '          note: |-',
    '            first',
    '            second',
    '    engines: {npm: "first\\nsecond"}',
    '',
  ].join('\n'))
  expect(load(serialized)).toStrictEqual(lockfile)
})

test('keeps trailing newlines in block scalars between packages', () => {
  const lockfile = {
    packages: {
      first: { deprecated: 'old\n\n' },
      second: { deprecated: '\n' },
    },
    snapshots: {},
  }

  expect(load(lockfileYamlDump(lockfile))).toStrictEqual(lockfile)
})

test('numeric-looking strings read back as strings, as values and as importer keys', () => {
  const values = ['+.5', '-.5', '1e999', '9'.repeat(400), '1_000', '0x1F', '2020-01-02']
  const lockfile = {
    lockfileVersion: '9.0',
    importers: Object.fromEntries(values.map((value) => [value, { dependencies: { foo: { specifier: value, version: value } } }])),
  }

  expect(loadLockfileYaml(lockfileYamlDump(lockfile))).toStrictEqual(lockfile)
})
