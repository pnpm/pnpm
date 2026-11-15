import { expect, test } from '@jest/globals'

import { getCliOptionsTypes, getCommandFullName, pnpmCmds } from '../src/cmd/index.js'
import { parseCliArgs } from '../src/parseCliArgs.js'

test('the "issues" alias resolves to the "bugs" command', async () => {
  expect(getCommandFullName('issues')).toBe('bugs')
  expect(pnpmCmds.issues).toBe(pnpmCmds.bugs)
  expect(getCliOptionsTypes('issues')).toHaveProperty(['registry'])
  expect(getCliOptionsTypes('issues')).toBe(getCliOptionsTypes('bugs'))

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
    ['--dir', 'pm', 'pm', 'clean'],
    ['--config.dir', '/tmp/pkg', 'pm', 'clean'],
    ['--config.dir=/tmp/pkg', 'pm', 'clean'],
    ['--config.lockfile', 'pm', 'clean'],
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
    [['--config.dir', 'pm', 'clean'], 'clean'],
    [['-C', 'pm', 'clean'], 'clean'],
    [['run', 'pm'], 'run'],
    [['exec', 'pm', 'clean'], 'exec'],
    [['with', '10', 'pm', 'clean'], 'with'],
    [['--', 'pm', 'clean'], 'run'],
    [['---', 'pm', 'clean'], 'run'],
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

  const configDirWithCurrent = await parseCliArgs(['--config.dir', '/tmp/pkg', 'with', 'current', 'list'])
  expect(configDirWithCurrent.cmd).toBe('list')
  expect(configDirWithCurrent.options.dir).toBe('/tmp/pkg')

  const notWithCurrent = await parseCliArgs(['-C', 'with', 'current', 'install'])
  expect(notWithCurrent.cmd).toBe('run')

  const separatorWithCurrent = await parseCliArgs(['---', 'with', 'current', 'install'])
  expect(separatorWithCurrent.cmd).toBe('with')
  expect(separatorWithCurrent.params).toStrictEqual(['current', 'install'])

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

test('command-specific value options before with current and pm resolve the target command', async () => {
  const configInitType = await parseCliArgs(['--config.init-type', 'commonjs', 'with', 'current', 'init'])
  expect(configInitType.cmd).toBe('init')
  expect(configInitType.params).toStrictEqual([])
  expect(configInitType.options['init-type']).toBe('commonjs')

  const directInitType = await parseCliArgs(['--init-type', 'commonjs', 'with', 'current', 'init'])
  expect(directInitType.cmd).toBe('init')
  expect(directInitType.params).toStrictEqual([])
  expect(directInitType.options['init-type']).toBe('commonjs')

  const forcedInitType = await parseCliArgs(['--init-type', 'commonjs', 'pm', 'init'])
  expect(forcedInitType.cmd).toBe('init')
  expect(forcedInitType.builtInCommandForced).toBe(true)
  expect(forcedInitType.options['init-type']).toBe('commonjs')

  const tripleDashDir = await parseCliArgs(['--dir', '---dir', 'with', 'current', 'list'])
  expect(tripleDashDir.cmd).toBe('list')
  expect(tripleDashDir.params).toStrictEqual([])
  expect(tripleDashDir.options.dir).toMatch(/(?:^|[\\/])---dir$/)

  const depthValueWithCurrent = await parseCliArgs(['--depth', '2', 'with', 'current', 'list'])
  expect(depthValueWithCurrent.cmd).toBe('list')
  expect(depthValueWithCurrent.options.depth).toBe(2)

  const missingDepthDirect = await parseCliArgs(['--depth', 'list'])
  expect(missingDepthDirect.cmd).toBe('list')

  const missingDepthWithCurrent = await parseCliArgs(['--depth', 'with', 'current', 'list'])
  expect(missingDepthWithCurrent.cmd).toBe('list')

  const missingDepthPm = await parseCliArgs(['--depth', 'pm', 'list'])
  expect(missingDepthPm.cmd).toBe('list')
  expect(missingDepthPm.builtInCommandForced).toBe(true)

  const listTargetNamedPm = await parseCliArgs(['--depth', 'list', 'pm'])
  expect(listTargetNamedPm.cmd).toBe('list')
  expect(listTargetNamedPm.builtInCommandForced).toBe(false)
  expect(listTargetNamedPm.params).toStrictEqual(['pm'])

  const fallbackScriptWithCurrent = await parseCliArgs([
    '--workspace-concurrency',
    '2',
    'with',
    'current',
    'build',
    '--help',
  ])
  expect(fallbackScriptWithCurrent.cmd).toBe('run')
  expect(fallbackScriptWithCurrent.fallbackCommandUsed).toBe(true)
  expect(fallbackScriptWithCurrent.options['workspace-concurrency']).toBe(2)
  expect(fallbackScriptWithCurrent.params).toStrictEqual(['build', '--help'])

  const markerCollision = await parseCliArgs([
    '--filter',
    '__pnpm_prefix_0_pkg',
    '--filter',
    '__pnpm_prefix_1_pkg',
    'pm',
    'clean',
  ])
  expect(markerCollision.cmd).toBe('clean')
  expect(markerCollision.builtInCommandForced).toBe(true)
  expect(markerCollision.options.filter).toStrictEqual([
    '__pnpm_prefix_0_pkg',
    '__pnpm_prefix_1_pkg',
  ])

  const dlxHelpForwarding = await parseCliArgs(['dlx', '--package', 'typescript', 'tsc', '--help'])
  expect(dlxHelpForwarding.cmd).toBe('dlx')
  expect(dlxHelpForwarding.options.package).toStrictEqual(['typescript'])
  expect(dlxHelpForwarding.params).toStrictEqual(['tsc', '--help'])
})
