import chalk from 'chalk'
import { getUpdateChoices } from '../../lib/update/getUpdateChoices'

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
          homepage: 'https://pnpm.io/',
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
          repository: {
            url: 'git://github.com/pnpm/pnpm.git',
          },
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
    ], false))
    .toStrictEqual([
      {
        name: '[dependencies]',
        message: 'dependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL              ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: chalk`foo                                                          1.0.0 ❯ {redBright.bold 2.0.0}             https://pnpm.io/ `,
            value: 'foo',
            name: 'foo',
          },
        ],
      },
      {
        name: '[devDependencies]',
        message: 'devDependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: chalk`qar                                                          1.0.0 ❯ 1.{yellowBright.bold 2.0}                 `,
            name: 'qar',
            value: 'qar',
          },
          {
            message: chalk`zoo                                                          1.1.0 ❯ 1.{yellowBright.bold 2.0}                 `,
            name: 'zoo',
            value: 'zoo',
          },
          {
            message: chalk`foo                                                          1.0.1 ❯ 1.{yellowBright.bold 2.0}                 `,
            name: 'foo',
            value: 'foo',
          },
        ],
      },
      {
        name: '[optionalDependencies]',
        message: 'optionalDependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: chalk`qaz                                                          1.0.1 ❯ 1.{yellowBright.bold 2.0}                 `,
            name: 'qaz',
            value: 'qaz',
          },
        ],
      },
    ])
})
