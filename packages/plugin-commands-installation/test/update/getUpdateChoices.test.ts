import getUpdateChoices from '../../lib/update/getUpdateChoices'
import chalk = require('chalk')

test('getUpdateChoices()', () => {
  expect(
    getUpdateChoices([
      {
        alias: 'foo',
        belongsTo: 'dependencies' as const,
        current: '1.0.0',
        latestManifest: {
          name: 'foo',
          version: '2.0.0',
        },
        packageName: 'foo',
        wanted: '1.0.0',
      },
      {
        alias: 'foo',
        belongsTo: 'devDependencies' as const,
        current: '1.0.0',
        latestManifest: {
          name: 'foo',
          version: '2.0.0',
        },
        packageName: 'foo',
        wanted: '1.0.0',
      },
      {
        alias: 'qar',
        belongsTo: 'devDependencies' as const,
        current: '1.0.0',
        latestManifest: {
          name: 'qar',
          version: '1.2.0',
        },
        packageName: 'qar',
        wanted: '1.0.0',
      },
      {
        alias: 'zoo',
        belongsTo: 'devDependencies' as const,
        current: '1.1.0',
        latestManifest: {
          name: 'zoo',
          version: '1.2.0',
        },
        packageName: 'zoo',
        wanted: '1.1.0',
      },
      {
        alias: 'qaz',
        belongsTo: 'optionalDependencies' as const,
        current: '1.0.1',
        latestManifest: {
          name: 'qaz',
          version: '1.2.0',
        },
        packageName: 'qaz',
        wanted: '1.0.1',
      },
      {
        alias: 'qaz',
        belongsTo: 'devDependencies' as const,
        current: '1.0.1',
        latestManifest: {
          name: 'qaz',
          version: '1.2.0',
        },
        packageName: 'foo',
        wanted: '1.0.1',
      },
    ]))
    .toStrictEqual([
      {
        message: chalk`foo            1.0.0 ❯ {redBright.bold 2.0.0} \n    foo (dev)      1.0.1 ❯ 1.{yellowBright.bold 2.0} `,
        name: 'foo',
      },
      {
        message: chalk`qar (dev)      1.0.0 ❯ 1.{yellowBright.bold 2.0} `,
        name: 'qar',
      },
      {
        message: chalk`qaz (optional) 1.0.1 ❯ 1.{yellowBright.bold 2.0} `,
        name: 'qaz',
      },
      {
        message: chalk`zoo (dev)      1.1.0 ❯ 1.{yellowBright.bold 2.0} `,
        name: 'zoo',
      },
    ])
})
