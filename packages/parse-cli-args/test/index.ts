import os from 'os'
import PnpmError from '@pnpm/error'
import parseCliArgs from '@pnpm/parse-cli-args'
import tempy from 'tempy'

const DEFAULT_OPTS = {
  getCommandLongName: (commandName: string) => commandName,
  getTypesByCommandName: (commandName: string) => ({}),
  renamedOptions: { prefix: 'dir' },
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

test('when runnning a global command inside a workspace, the workspace should be ignored', async () => {
  const { workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { global: Boolean },
  }, ['--global', 'add', 'foo'])
  expect(workspaceDir).toBeFalsy()
})

test('when runnning with --ignore-workspace option inside a workspace, the workspace should be ignored', async () => {
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
  }, ['install', '--save-dev', '--registry=https://example.com', '--qar', '--filter=packages'])
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
  expect(cmd).toBe(null)
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

test('any unknown command is treated as a script', async () => {
  const { options, cmd, params, fallbackCommandUsed } = await parseCliArgs({
    ...DEFAULT_OPTS,
    fallbackCommand: 'run',
    getCommandLongName: () => null,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['foo', '--recursive'])
  expect(cmd).toBe('run')
  expect(params).toStrictEqual(['foo'])
  expect(options).toHaveProperty(['recursive'])
  expect(fallbackCommandUsed).toBeTruthy()
})

test("don't use the fallback command if no command is present", async () => {
  const { cmd, params } = await parseCliArgs({
    ...DEFAULT_OPTS,
    fallbackCommand: 'run',
    getCommandLongName: () => null,
    universalOptionsTypes: { filter: [String, Array] },
  }, [])
  expect(cmd).toBe(null)
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
  process.chdir(tempy.directory())
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
