import { expect, test } from '@jest/globals'
import { load } from 'js-yaml'

import { lockfileYamlDump } from '../src/lockfileYamlDump.js'

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
