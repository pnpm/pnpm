import os from 'os'
import { type PnpmError } from '@pnpm/error'
import { parseCliArgs } from '@pnpm/parse-cli-args'
import { temporaryDirectory } from 'tempy'

const DEFAULT_OPTS = {
  getCommandLongName: (_commandName: string) => _commandName,
  getTypesByCommandName: (_commandName: string) => ({}),
  renamedOptions: { prefix: 'dir' },
  subcommandsByCommandName: {},
  shorthandsByCommandName: {},
  universalOptionsTypes: {},
  universalShorthands: {},
}

test('a command is recursive if it has a --filter option', async () => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['--filter', 'foo', 'update'])
  expect(cmd).toBe('update')
  expect(options).toHaveProperty(['recursive'])
})

test('a command is recursive if it has a --filter-prod option', async () => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { 'filter-prod': [String, Array] },
  }, ['--filter-prod', 'foo', 'update'])
  expect(cmd).toBe('update')
  expect(options).toHaveProperty(['recursive'])
})

test('a command is recursive if -r option is used', async () => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { recursive: Boolean },
    universalShorthands: { r: '--recursive' },
  }, ['-r', 'update'])
  expect(cmd).toBe('update')
  expect(options).toHaveProperty(['recursive'])
})

test('a command is recursive if --recursive option is used', async () => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { recursive: Boolean },
  }, ['-r', 'update'])
  expect(cmd).toBe('update')
  expect(options).toHaveProperty(['recursive'])
})

test('recursive is returned as the command name if no subcommand passed', async () => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['recursive'])
  expect(cmd).toBe('recursive')
  expect(options).toHaveProperty(['recursive'])
})

test('when running a global command inside a workspace, the workspace should be ignored', async () => {
  const { workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { global: Boolean },
  }, ['--global', 'add', 'foo'])
  expect(workspaceDir).toBeFalsy()
})

test('when running with --ignore-workspace option inside a workspace, the workspace should be ignored', async () => {
  const { workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { global: Boolean },
  }, ['--ignore-workspace', 'add', 'foo'])
  expect(workspaceDir).toBeFalsy()
})

test('command is used recursively', async () => {
  const { cmd, options } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: {},
  }, ['recursive', 'outdated'])
  expect(cmd).toBe('outdated')
  expect(options.recursive).toBe(true)
})

test('the install command is converted to add when called with args', async () => {
  const { params, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['install', 'rimraf@1'])
  expect(cmd).toBe('add')
  expect(params).toStrictEqual(['rimraf@1'])
})

test('the "i" command is converted to add when called with args', async () => {
  const { params, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getCommandLongName: (commandName) => commandName === 'i' ? 'install' : commandName,
  }, ['i', 'rimraf@1'])
  expect(cmd).toBe('add')
  expect(params).toStrictEqual(['rimraf@1'])
})

test('detect unknown options', async () => {
  const { unknownOptions } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getTypesByCommandName: (commandName: string) => {
      if (commandName === 'install') {
        return {
          bar: Boolean,
          recursive: Boolean,
          registry: String,
        }
      }
      return {}
    },
    universalOptionsTypes: { filter: [String, Array] },
  }, ['install', '--save-dev', '--registry=https://example.com', '--@scope:registry=https://scope.example.com/npm', '--qar', '--filter=packages'])
  expect(Array.from(unknownOptions.entries())).toStrictEqual([['save-dev', []], ['qar', ['bar']]])
})

test('allow any option that starts with "config."', async () => {
  const { options, unknownOptions } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getTypesByCommandName: (commandName: string) => {
      if (commandName === 'install') {
        return {
          bar: Boolean,
          recursive: Boolean,
          registry: String,
        }
      }
      return {}
    },
    universalOptionsTypes: { filter: [String, Array] },
  }, ['install', '--config.save-dev', '--registry=https://example.com', '--config.qar', '--filter=packages'])
  expect(Array.from(unknownOptions.entries())).toStrictEqual([])
  expect(options.qar).toBe(true)
  expect(options['save-dev']).toBe(true)
})

test('do not incorrectly change "install" command to "add"', async () => {
  const { cmd, fallbackCommandUsed } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getTypesByCommandName: (commandName: string) => {
      switch (commandName) {
      case 'install': return { 'network-concurrency': Number }
      default: return {}
      }
    },
    universalOptionsTypes: {
      prefix: String,
    },
    universalShorthands: {
      C: '--prefix',
      r: '--recursive',
    },
  }, ['install', '-C', os.homedir(), '--network-concurrency', '1'])
  expect(cmd).toBe('install')
  expect(fallbackCommandUsed).toBeFalsy()
})

test('if a help option is used, set cmd to "help"', async () => {
  const { cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['install', '--help'])
  expect(cmd).toBe('help')
})

test('if a help option is used with an unknown command, do not set cmd to "help"', async () => {
  const { cmd, fallbackCommandUsed } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getCommandLongName: () => null,
    fallbackCommand: 'run',
  }, ['eslint', '--help'])
  expect(cmd).toBe('run')
  expect(fallbackCommandUsed).toBeTruthy()
})

test('no command', async () => {
  const { cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['--version'])
  expect(cmd).toBeNull()
})

test('use command-specific shorthands', async () => {
  const { options } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getTypesByCommandName: (commandName: string) => {
      if (commandName === 'install') {
        return {
          dev: Boolean,
        }
      }
      return {}
    },
    shorthandsByCommandName: {
      install: { D: '--dev' },
    },
  }, ['install', '-D'])
  expect(options).toHaveProperty(['dev'])
})

test('command-specific shorthands override universal shorthands', async () => {
  const { options } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getTypesByCommandName: (commandName: string) => {
      if (commandName === 'add') {
        return {
          'save-dev': Boolean,
          'save-prod': Boolean,
          'save-optional': Boolean,
          'save-exact': Boolean,
          loglevel: String,
          parseable: Boolean,
        }
      }
      return {}
    },
    universalShorthands: {
      d: '--loglevel',
      p: '--parseable',
    },
    shorthandsByCommandName: {
      add: {
        d: '--save-dev',
        p: '--save-prod',
        o: '--save-optional',
        e: '--save-exact',
      },
    },
  }, ['add', '-d', '-p', '-o', '-e', 'package'])
  expect(options['save-dev']).toBe(true)
  expect(options['save-prod']).toBe(true)
  expect(options['save-optional']).toBe(true)
  expect(options['save-exact']).toBe(true)
  expect(options.loglevel).toBeUndefined()
  expect(options.parseable).toBeUndefined()
})

test('any unknown command is treated as a script', async () => {
  const { options, cmd, params, fallbackCommandUsed } = await parseCliArgs({
    ...DEFAULT_OPTS,
    fallbackCommand: 'run',
    getCommandLongName: () => null,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['--recursive', 'foo'])
  expect(cmd).toBe('run')
  expect(params).toStrictEqual(['foo'])
  expect(options).toHaveProperty(['recursive'])
  expect(fallbackCommandUsed).toBeTruthy()
})

test('run script with --help before script name is help command', async () => {
  const { cmd, params } = await parseCliArgs({
    ...DEFAULT_OPTS,
    fallbackCommand: 'run',
  }, ['run', '--help', 'foo'])
  expect(cmd).toBe('help')
  expect(params).toStrictEqual(['run', 'foo'])
})

test.each([
  ['foo', { params: 'foo', options: {} }],
  ['foo --bar baz --qux', { params: 'foo --bar baz --qux', options: {} }],
  ['-r foo', { params: 'foo', options: { recursive: true } }],
  ['-r foo --bar baz --qux', { params: 'foo --bar baz --qux', options: { recursive: true } }],

  // Edge case where option value is the script name. Fortunately nopt handles this correctly.
  ['--test-pattern test test foo', { params: 'test foo', options: { 'test-pattern': ['test'] } }],

  // Ensure even builtin flags are passed to the script.
  ['foo -r', { params: 'foo -r', options: {} }],
  ['foo --recursive', { params: 'foo --recursive', options: {} }],
  ['foo -h', { params: 'foo -h', options: {} }],
  ['foo --help', { params: 'foo --help', options: {} }],
  ['foo --filter=bar', { params: 'foo --filter=bar', options: {} }],
])('run script arguments are correct for: %s', async (testInput, expected) => {
  for (const testWithCommandFallback of [true, false]) {
    // Whether or not the leading "run" portion of the command is written
    // shouldn't affect its arg parsing. Test both scenarios for good measure.
    const input = [...(testWithCommandFallback ? [] : ['run']), ...testInput.split(' ')]

    // eslint-disable-next-line no-await-in-loop
    const { options, cmd, params, fallbackCommandUsed } = await parseCliArgs({
      ...DEFAULT_OPTS,
      fallbackCommand: 'run',
      getCommandLongName: (name) => name === 'run' ? 'run' : null,
      universalOptionsTypes: { filter: [String, Array], 'test-pattern': [String, Array] },
    }, input)
    expect(cmd).toBe('run')
    expect(params).toStrictEqual(expected.params.split(' '))
    expect(options).toStrictEqual(expected.options)

    if (testWithCommandFallback) {
      expect(fallbackCommandUsed).toBeTruthy()
    }
  }
})

test("don't use the fallback command if no command is present", async () => {
  const { cmd, params } = await parseCliArgs({
    ...DEFAULT_OPTS,
    fallbackCommand: 'run',
    getCommandLongName: () => null,
    universalOptionsTypes: { filter: [String, Array] },
  }, [])
  expect(cmd).toBeNull()
  expect(params).toStrictEqual([])
})

test('--workspace-root changes the directory to the workspace root', async () => {
  const { options, workspaceDir } = await parseCliArgs({ ...DEFAULT_OPTS }, ['--workspace-root'])
  expect(workspaceDir).toBeTruthy()
  expect(options.dir).toBe(workspaceDir)
})

test('--workspace-root fails if used with --global', async () => {
  let err!: PnpmError
  try {
    await parseCliArgs({ ...DEFAULT_OPTS }, ['--workspace-root', '--global'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_OPTIONS_CONFLICT')
})

test('--workspace-root fails if used outside of a workspace', async () => {
  process.chdir(temporaryDirectory())
  let err!: PnpmError
  try {
    await parseCliArgs({ ...DEFAULT_OPTS }, ['--workspace-root'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_NOT_IN_WORKSPACE')
})

test('everything after an escape arg is a parameter', async () => {
  const { params, options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    escapeArgs: ['exec'],
  }, ['-r', 'exec', 'rm', '-rf', 'node_modules'])
  expect(cmd).toBe('exec')
  expect(options).toHaveProperty(['recursive'])
  expect(params).toStrictEqual(['rm', '-rf', 'node_modules'])
})

test('everything after an escape arg is a parameter, even if it has a help option', async () => {
  const { params, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    escapeArgs: ['exec'],
  }, ['exec', 'rm', '--help'])
  expect(cmd).toBe('exec')
  expect(params).toStrictEqual(['rm', '--help'])
})

test('`pnpm install ""` is going to be just `pnpm install`', async () => {
  const { params, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['install', ''])
  expect(cmd).toBe('add')
  // empty string in params will be filtered at: https://github.com/pnpm/pnpm/blob/main/pkg-manager/plugin-commands-installation/src/installDeps.ts#L196
  expect(params).toStrictEqual([''])
})

test('should not swallows empty string in params', async () => {
  const { params, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['run', 'echo', '', 'foo', '', 'bar'])
  expect(cmd).toBe('run')
  expect(params).toStrictEqual(['echo', '', 'foo', '', 'bar'])
})

test('dlx parses CLI options in between "dlx" and the command name', async () => {
  const { params, options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, [
    '--reporter=append-only',
    'dlx',
    '--allow-build=some-package',
    '--package=some-bin-package',
    'some-command',
    '--this-is-not-a-flag',
    'another-argument',
  ])
  expect(cmd).toBe('dlx')
  expect(options).toStrictEqual({
    reporter: 'append-only',
    'allow-build': 'some-package',
    package: 'some-bin-package',
  })
  expect(params).toStrictEqual([
    'some-command',
    '--this-is-not-a-flag',
    'another-argument',
  ])
})

test('dlx stops parsing after "--"', async () => {
  const { params, options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, [
    'dlx',
    '--package=some-package',
    '--allow-build=foo',
    '--allow-build=bar',
    '--',
    '--this-is-a-command',
    'argument',
  ])
  expect(cmd).toBe('dlx')
  expect(options).toStrictEqual({
    package: 'some-package',
    'allow-build': ['foo', 'bar'],
  })
  expect(params).toStrictEqual([
    '--this-is-a-command',
    'argument',
  ])
})
