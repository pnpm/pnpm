import { stripVTControlCharacters } from 'util'
import chalk from 'chalk'
import { getUpdateChoices } from '../../lib/update/getUpdateChoices.js'

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
            message: `foo                                                          1.0.0 ❯ ${chalk.redBright.bold('2.0.0')}             https://pnpm.io/ `,
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
            message: `qar                                                          1.0.0 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'qar',
            value: 'qar',
          },
          {
            message: `zoo                                                          1.1.0 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'zoo',
            value: 'zoo',
          },
          {
            message: `foo                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
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
            message: `qaz                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'qaz',
            value: 'qaz',
          },
        ],
      },
    ])
})

test('getUpdateChoices() handles long version strings without wrapping', () => {
  const choices = getUpdateChoices([
    {
      alias: '@typescript/native-preview',
      belongsTo: 'devDependencies' as const,
      current: '7.0.0-dev.20251209.1',
      latestManifest: {
        name: '@typescript/native-preview',
        version: '7.0.0-dev.20251214.1',
        homepage: 'https://github.com/nicolo-ribaudo/tc39-proposal-structs',
      },
      packageName: '@typescript/native-preview',
      wanted: '7.0.0-dev.20251209.1',
    },
  ], false)

  const dataRow = choices[0].choices[1] as { message: string; value: string; name: string }
  expect(dataRow).toStrictEqual({
    message: expect.stringContaining('7.0.0-dev.20251209.1'),
    value: '@typescript/native-preview',
    name: '@typescript/native-preview',
  })
  // The rendered message must be a single line (no wrapping)
  expect(dataRow.message).not.toContain('\n')
  // Both current and target versions must appear in the output.
  // Strip ANSI codes first because colorizeSemverDiff embeds color escapes
  // within the version string, which would break a plain substring match
  // when chalk has colors enabled.
  expect(stripVTControlCharacters(dataRow.message)).toContain('7.0.0-dev.20251214.1')
})
