import { expect, test } from '@jest/globals'

import { getCliOptionsTypes, getCommandFullName, pnpmCmds } from '../src/cmd/index.js'
import { parseCliArgs } from '../src/parseCliArgs.js'

test('the "issues" alias resolves to the "bugs" command', async () => {
  expect(getCommandFullName('issues')).toBe('bugs')
  expect(pnpmCmds.issues).toBe(pnpmCmds.bugs)
  expect(getCliOptionsTypes('issues')).toHaveProperty(['registry'])

  const { cmd } = await parseCliArgs(['issues', 'is-positive'])
  expect(cmd).toBe('bugs')
})

test('names of Object.prototype members are not commands', async () => {
  expect(getCommandFullName('constructor')).toBeNull()
  expect(getCommandFullName('__proto__')).toBeNull()
  expect(getCliOptionsTypes('constructor')).toEqual({})

  const { cmd } = await parseCliArgs(['constructor'])
  expect(cmd).toBe('run')
})

test('a bare --fix reaches the audit command handler as an empty string', async () => {
  const { options } = await parseCliArgs(['audit', '--fix'])
  expect(options.fix).toBe('')
})

test('a bare --fix does not consume the flag that follows it', async () => {
  const { options } = await parseCliArgs(['audit', '--fix', '--json'])
  expect(options.fix).toBe('')
  expect(options.json).toBe(true)
})

test('remove takes --unsafe-perm in every spelling, before and after the command', async () => {
  const enabled = await Promise.all([
    ['remove', 'foo', '--unsafe-perm'],
    ['--unsafe-perm', 'remove', 'foo'],
    ['remove', 'foo', '--unsafe-perm=true'],
  ].map((argv) => parseCliArgs(argv)))
  for (const { cmd, params, options, unknownOptions } of enabled) {
    expect(cmd).toBe('remove')
    expect(params).toStrictEqual(['foo'])
    expect(options['unsafe-perm']).toBe(true)
    expect(unknownOptions.size).toBe(0)
  }

  const disabled = await Promise.all([
    ['remove', 'foo', '--no-unsafe-perm'],
    ['remove', 'foo', '--unsafe-perm=false'],
  ].map((argv) => parseCliArgs(argv)))
  for (const { params, options, unknownOptions } of disabled) {
    expect(params).toStrictEqual(['foo'])
    expect(options['unsafe-perm']).toBe(false)
    expect(unknownOptions.size).toBe(0)
  }
})

test('pm prefix is recognized after pre-command flags and with current, but not as an option value or command arg', async () => {
  const forcedCases = await Promise.all([
    ['pm', 'clean'],
    ['--dir', '/tmp/pkg', 'pm', 'clean'],
    ['-rC', '/tmp/pkg', 'pm', 'clean'],
    ['--lockfile', 'pm', 'clean'],
    ['with', 'current', 'pm', 'clean'],
    ['pm', 'with', 'current', 'clean'],
    ['recursive', 'pm', 'clean'],
    ['--dir', '/tmp/pkg', 'with', 'current', 'pm', 'clean'],
  ].map((argv) => parseCliArgs(argv)))
  for (const parsed of forcedCases) {
    expect(parsed.cmd).toBe('clean')
    expect(parsed.builtInCommandForced).toBe(true)
  }

  const unforcedExpectations = [
    [['--dir', 'pm', 'clean'], 'clean'],
    [['-C', 'pm', 'clean'], 'clean'],
    [['run', 'pm'], 'run'],
    [['exec', 'pm', 'clean'], 'exec'],
    [['with', '10', 'pm', 'clean'], 'with'],
    [['--', 'pm', 'clean'], 'run'],
  ] as const
  const unforcedCases = await Promise.all(
    unforcedExpectations.map(async ([argv, expectedCmd]) => ({
      parsed: await parseCliArgs([...argv]),
      expectedCmd,
    }))
  )
  for (const { parsed, expectedCmd } of unforcedCases) {
    expect(parsed.cmd).toBe(expectedCmd)
    expect(parsed.builtInCommandForced).toBe(false)
  }
})

test('with current works after subcommand boolean flags and rejects missing inner commands', async () => {
  const bareInit = await parseCliArgs(['--bare', 'with', 'current', 'init'])
  expect(bareInit.cmd).toBe('init')
  expect(bareInit.options.bare).toBe(true)

  const recursiveWithCurrent = await parseCliArgs(['recursive', 'with', 'current', 'list'])
  expect(recursiveWithCurrent.cmd).toBe('list')
  expect(recursiveWithCurrent.options.recursive).toBe(true)

  const notWithCurrent = await parseCliArgs(['-C', 'with', 'current', 'install'])
  expect(notWithCurrent.cmd).toBe('run')

  await Promise.all([
    ['with', 'current'],
    ['pm', 'with', 'current'],
    ['with', 'current', 'pm'],
    ['--bare', 'with', 'current'],
  ].map((incomplete) =>
    expect(parseCliArgs(incomplete)).rejects.toMatchObject({
      code: 'ERR_PNPM_MISSING_WITH_CURRENT_CMD',
    })
  ))
})
