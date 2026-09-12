import { stripVTControlCharacters } from 'node:util'

import { expect, test } from '@jest/globals'
import chalk from 'chalk'
import stringWidth from 'string-width'

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
            message: 'Package                                                    Current   Target            URL              ',
            disabled: true,
            hint: '',
            short: '',
            value: '',
          },
          {
            message: `foo                                                          1.0.0 ❯ ${chalk.redBright.bold('2.0.0')}             https://pnpm.io/ `,
            value: 'foo',
            name: 'foo',
            short: 'foo',
          },
        ],
      },
      {
        name: '[devDependencies]',
        message: 'devDependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL ',
            message: 'Package                                                    Current   Target            URL ',
            disabled: true,
            hint: '',
            short: '',
            value: '',
          },
          {
            message: `qar                                                          1.0.0 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'qar',
            short: 'qar',
            value: 'qar',
          },
          {
            message: `zoo                                                          1.1.0 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'zoo',
            short: 'zoo',
            value: 'zoo',
          },
          {
            message: `foo                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'foo',
            short: 'foo',
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
            message: 'Package                                                    Current   Target            URL ',
            disabled: true,
            hint: '',
            short: '',
            value: '',
          },
          {
            message: `qaz                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'qaz',
            short: 'qaz',
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

  const dataRow = choices[0].choices[1] as { message: string; value: string; name: string; short: string }
  expect(dataRow).toStrictEqual({
    message: expect.stringContaining('7.0.0-dev.20251209.1'),
    value: '@typescript/native-preview',
    name: '@typescript/native-preview',
    short: '@typescript/native-preview',
  })
  // The rendered message must be a single line (no wrapping)
  expect(dataRow.message).not.toContain('\n')
  // Both current and target versions must appear in the output.
  // Strip ANSI codes first because colorizeSemverDiff embeds color escapes
  // within the version string, which would break a plain substring match
  // when chalk has colors enabled.
  expect(stripVTControlCharacters(dataRow.message)).toContain('7.0.0-dev.20251214.1')
})

test('getUpdateChoices() sizes the version columns by their rendered width', () => {
  const outdated = (alias: string, current: string) => ({
    alias,
    belongsTo: 'dependencies' as const,
    current,
    latestManifest: { name: alias, version: '2.0.0' },
    packageName: alias,
    wanted: current,
  })

  // A version made of double-width characters occupies twice the columns
  // its code units suggest. Sizing the column by code units leaves the
  // cell wider than the column it is laid out in, which wraps the row.
  const choices = getUpdateChoices([outdated('a', '1.0.0-中文中文中文中文中文'), outdated('b', '1.0.0')], false)

  const rows = choices[0].choices.map((choice) => stripVTControlCharacters((choice as { message: string }).message))
  for (const row of rows) {
    expect(row).not.toContain('\n')
  }
  const arrowColumns = rows.slice(1).map((row) => stringWidth(row.split('❯')[0]))
  expect(arrowColumns[0]).toBe(arrowColumns[1])
})

test('getUpdateChoices() groups GitHub Actions separately', () => {
  const choices = getUpdateChoices([{
    alias: 'actions/checkout',
    belongsTo: 'devDependencies',
    current: '4.1.0',
    dependencyType: 'githubAction',
    latestManifest: {
      name: 'actions/checkout',
      version: '4.2.2',
      homepage: 'https://github.com/actions/checkout',
    },
    packageName: 'actions/checkout',
    wanted: '4.2.2',
  }], false)

  expect(choices).toHaveLength(1)
  expect(choices[0].message).toBe('GitHub Actions')
  expect(choices[0].choices[1]).toMatchObject({ name: 'actions/checkout', value: 'actions/checkout' })
})

test('getUpdateChoices() names every workspace a collapsed choice came from', () => {
  const outdated = (workspace: string) => ({
    alias: 'foo',
    belongsTo: 'dependencies' as const,
    current: '1.0.0',
    latestManifest: { name: 'foo', version: '2.0.0' },
    packageName: 'foo',
    wanted: '1.0.0',
    workspace,
  })

  const choices = getUpdateChoices([outdated('web'), outdated('tooling')], true)

  // Selecting the choice updates the package in every project, so the two
  // entries stay one row — but the row has to say which projects it covers.
  expect(choices).toHaveLength(1)
  expect(choices[0].choices).toHaveLength(2)
  const dataRow = choices[0].choices[1] as { message: string }
  expect(stripVTControlCharacters(dataRow.message)).toContain('web, tooling')
})

test('getUpdateChoices() strips control and formatting characters from labels it renders', () => {
  const choices = getUpdateChoices([
    {
      alias: 'foo\u202E',
      belongsTo: 'dependencies' as const,
      current: '1.0.0',
      latestManifest: {
        name: 'foo',
        version: '2.0.0',
        homepage: 'https://example.test/\u001b[2J\u2066\nEVIL',
      },
      packageName: 'foo\u202E',
      wanted: '1.0.0',
      workspace: 'web\u001b[31m\u2069\nEVIL',
    },
  ], true)

  const dataRow = choices[0].choices[1] as { message: string, short: string, value: string }
  // Once the escape byte is gone the remainder is inert text, so what
  // matters is that no escape or newline reaches the prompt. The
  // colorized target carries escapes of its own by design, hence the
  // check on the workspace and URL cells rather than the whole row.
  const cells = dataRow.message.split('❯')[1]
  expect(cells).not.toContain('\u001b')
  expect(dataRow.message).not.toContain('\n')
  expect(dataRow.message).not.toMatch(/[\u202E\u2066\u2069]/u)
  expect(dataRow.message).toContain('https://example.test/')
  expect(dataRow.value).toBe('foo\u202E')
  expect(dataRow.short).toBe('foo')
})
