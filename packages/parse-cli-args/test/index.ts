import PnpmError from '@pnpm/error'
import parseCliArgs from '@pnpm/parse-cli-args'
import os = require('os')
import test = require('tape')
import tempy = require('tempy')

const DEFAULT_OPTS = {
  getCommandLongName: (commandName: string) => commandName,
  getTypesByCommandName: (commandName: string) => ({}),
  renamedOptions: { prefix: 'dir' },
  shorthandsByCommandName: {},
  universalOptionsTypes: {},
  universalShorthands: {},
}

test('a command is recursive if it has a --filter option', async (t) => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['--filter', 'foo', 'update'])
  t.equal(cmd, 'update')
  t.ok(options['recursive'])
  t.end()
})

test('a command is recursive if -r option is used', async (t) => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { recursive: Boolean },
    universalShorthands: { r: '--recursive' },
  }, ['-r', 'update'])
  t.equal(cmd, 'update')
  t.ok(options['recursive'])
  t.end()
})

test('a command is recursive if --recursive option is used', async (t) => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { recursive: Boolean },
  }, ['-r', 'update'])
  t.equal(cmd, 'update')
  t.ok(options['recursive'])
  t.end()
})

test('recursive is returned as the command name if no subcommand passed', async (t) => {
  const { options, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['recursive'])
  t.equal(cmd, 'recursive')
  t.ok(options['recursive'])
  t.end()
})

test('when runnning a global command inside a workspace, the workspace should be ignored', async (t) => {
  const { workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { global: Boolean },
  }, ['--global', 'add', 'foo'])
  t.notOk(workspaceDir)
  t.end()
})

test('command is used recursively', async (t) => {
  const { cmd, options } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: {},
  }, ['recursive', 'outdated'])
  t.equal(cmd, 'outdated')
  t.equal(options.recursive, true)
  t.end()
})

test('the install command is converted to add when called with args', async (t) => {
  const { params, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['install', 'rimraf@1'])
  t.equal(cmd, 'add')
  t.deepEqual(params, ['rimraf@1'])
  t.end()
})

test('the "i" command is converted to add when called with args', async (t) => {
  const { params, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getCommandLongName: (commandName) => commandName === 'i' ? 'install' : commandName,
  }, ['i', 'rimraf@1'])
  t.equal(cmd, 'add')
  t.deepEqual(params, ['rimraf@1'])
  t.end()
})

test('detect unknown options', async (t) => {
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
  t.deepEqual(
    Array.from(unknownOptions.entries()),
    [['save-dev', []], ['qar', ['bar']]]
  )
  t.end()
})

test('allow any option that starts with "config."', async (t) => {
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
  t.deepEqual(Array.from(unknownOptions.entries()), [])
  t.equal(options.qar, true)
  t.equal(options['save-dev'], true)
  t.end()
})

test('do not incorrectly change "install" command to "add"', async (t) => {
  const { cmd } = await parseCliArgs({
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
  t.equal(cmd, 'install')
  t.end()
})

test('if a help option is used, set cmd to "help"', async (t) => {
  const { cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['install', '--help'])
  t.equal(cmd, 'help')
  t.end()
})

test('no command', async (t) => {
  const { cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['--version'])
  t.equal(cmd, null)
  t.end()
})

test('use command-specific shorthands', async (t) => {
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
  t.ok(options['dev'])
  t.end()
})

test('any unknown command is treated as a script', async (t) => {
  const { options, cmd, params } = await parseCliArgs({
    ...DEFAULT_OPTS,
    fallbackCommand: 'run',
    getCommandLongName: () => null,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['foo', '--recursive'])
  t.equal(cmd, 'run')
  t.deepEqual(params, ['foo'])
  t.ok(options['recursive'])
  t.end()
})

test("don't use the fallback command if no command is present", async (t) => {
  const { cmd, params } = await parseCliArgs({
    ...DEFAULT_OPTS,
    fallbackCommand: 'run',
    getCommandLongName: () => null,
    universalOptionsTypes: { filter: [String, Array] },
  }, [])
  t.equal(cmd, null)
  t.deepEqual(params, [])
  t.end()
})

test('--workspace-root changes the directory to the workspace root', async (t) => {
  const { options, workspaceDir } = await parseCliArgs({ ...DEFAULT_OPTS }, ['--workspace-root'])
  t.ok(workspaceDir)
  t.equal(options.dir, workspaceDir)
  t.end()
})

test('--workspace-root fails if used with --global', async (t) => {
  let err!: PnpmError
  try {
    await parseCliArgs({ ...DEFAULT_OPTS }, ['--workspace-root', '--global'])
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_OPTIONS_CONFLICT')
  t.end()
})

test('--workspace-root fails if used outside of a workspace', async (t) => {
  process.chdir(tempy.directory())
  let err!: PnpmError
  try {
    await parseCliArgs({ ...DEFAULT_OPTS }, ['--workspace-root'])
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_NOT_IN_WORKSPACE')
  t.end()
})
