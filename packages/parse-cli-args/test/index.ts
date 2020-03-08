import parseCliArgs from '@pnpm/parse-cli-args'
import os = require('os')
import path = require('path')
import test = require('tape')

const DEFAULT_OPTS = {
  getCommandLongName: (commandName: string) => commandName,
  getTypesByCommandName: (commandName: string) => ({}),
  isKnownCommand: (commandName: string) => true,
  renamedOptions: { 'prefix': 'dir' },
  shorthandsByCommandName: {},
  universalOptionsTypes: {},
  universalShorthands: {},
}

test('a command is recursive if it has a --filter option', async (t) => {
  const { cliConf, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['--filter', 'foo', 'update'])
  t.equal(cmd, 'update')
  t.ok(cliConf['recursive'])
  t.end()
})

test('a command is recursive if -r option is used', async (t) => {
  const { cliConf, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { recursive: Boolean },
    universalShorthands: { 'r': '--recursive' },
  }, ['-r', 'update'])
  t.equal(cmd, 'update')
  t.ok(cliConf['recursive'])
  t.end()
})

test('a command is recursive if --recursive option is used', async (t) => {
  const { cliConf, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { recursive: Boolean },
  }, ['-r', 'update'])
  t.equal(cmd, 'update')
  t.ok(cliConf['recursive'])
  t.end()
})

test('the install command is recursive when executed in a subdir of a workspace', async (t) => {
  const { cliConf, cmd, workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { dir: String },
  }, ['--dir', __dirname, 'install'])
  t.equal(cmd, 'install')
  t.ok(cliConf['recursive'])
  t.equal(workspaceDir, path.join(__dirname, '../../..'))
  t.end()
})

test('the install command is recursive when executed in the root of a workspace', async (t) => {
  const expectedWorkspaceDir = path.join(__dirname, '../../..')
  const { cliConf, cmd, workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { dir: String },
  }, ['--dir', expectedWorkspaceDir, 'install'])
  t.equal(cmd, 'install')
  t.ok(cliConf['recursive'])
  t.equal(workspaceDir, expectedWorkspaceDir)
  t.end()
})

test('recursive is returned as the command name if no subcommand passed', async (t) => {
  const { cliConf, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { filter: [String, Array] },
  }, ['recursive'])
  t.equal(cmd, 'recursive')
  t.ok(cliConf['recursive'])
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

test('isKnownCommand is false when an unknown command is used', async (t) => {
  const { cmd, isKnownCommand } = await parseCliArgs({
    ...DEFAULT_OPTS,
    isKnownCommand: () => false,
    universalOptionsTypes: {},
  }, ['foo'])
  t.false(isKnownCommand)
  t.equal(cmd, 'foo')
  t.end()
})

test('isKnownCommand is false when an unknown command is used recursively', async (t) => {
  const { cmd, isKnownCommand } = await parseCliArgs({
    ...DEFAULT_OPTS,
    isKnownCommand: () => false,
    universalOptionsTypes: {},
  }, ['recursive', 'foo'])
  t.false(isKnownCommand)
  t.equal(cmd, 'foo')
  t.end()
})

test('the install command is converted to add when called with args', async (t) => {
  const { cliArgs, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    isKnownCommand: (commandName) => commandName === 'install',
  }, ['install', 'rimraf@1'])
  t.equal(cmd, 'add')
  t.deepEqual(cliArgs, ['rimraf@1'])
  t.end()
})

test('the "i" command is converted to add when called with args', async (t) => {
  const { cliArgs, cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getCommandLongName: (commandName) => commandName === 'i' ? 'install' : commandName,
    isKnownCommand: (commandName) => commandName === 'install',
  }, ['i', 'rimraf@1'])
  t.equal(cmd, 'add')
  t.deepEqual(cliArgs, ['rimraf@1'])
  t.end()
})

test('detect unknown options', async (t) => {
  const { unknownOptions } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getTypesByCommandName: (commandName: string) => {
      if (commandName === 'install') {
        return {
          recursive: Boolean,
          registry: String,
        }
      }
      return {}
    },
    isKnownCommand: (commandName) => commandName === 'install',
    universalOptionsTypes: { filter: [String, Array] },
  }, ['install', '--save-dev', '--registry=https://example.com', '--qar', '--filter=packages'])
  t.deepEqual(unknownOptions, ['save-dev', 'qar'])
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
    isKnownCommand: (commandName) => commandName === 'install',
    universalOptionsTypes: {
      prefix: String,
    },
    universalShorthands: {
      'C': '--prefix',
      'r': '--recursive',
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
  const { cmd, isKnownCommand } = await parseCliArgs({
    ...DEFAULT_OPTS,
  }, ['--version'])
  t.equal(cmd, null)
  t.true(isKnownCommand)
  t.end()
})

test('use command-specific shorthands', async (t) => {
  const { cliConf } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getTypesByCommandName: (commandName: string) => {
      if (commandName === 'install') {
        return {
          'dev': Boolean,
        }
      }
      return {}
    },
    isKnownCommand: (commandName) => commandName === 'install',
    shorthandsByCommandName: {
      install: { D: '--dev' },
    },
  }, ['install', '-D'])
  t.ok(cliConf['dev'])
  t.end()
})
