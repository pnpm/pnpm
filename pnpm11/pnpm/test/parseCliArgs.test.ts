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
