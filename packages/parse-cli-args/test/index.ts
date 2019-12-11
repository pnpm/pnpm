import parseCliArgs from '@pnpm/parse-cli-args'
import os = require('os')
import path = require('path')
import test = require('tape')

const DEFAULT_OPTS = {
  getCommandLongName: (commandName: string) => commandName,
  getTypesByCommandName: (commandName: string) => ({}),
  globalOptionsTypes: {},
  isKnownCommand: (commandName: string) => true,
  renamedOptions: { 'prefix': 'dir' },
  shortHands: {},
}

test('a command is recursive if it has a --filter option', async (t) => {
  const { cmd, subCmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    globalOptionsTypes: { filter: [String, Array] },
  }, ['--filter', 'foo', 'update'])
  t.equal(cmd, 'recursive')
  t.equal(subCmd, 'update')
  t.end()
})

test('a command is recursive if -r option is used', async (t) => {
  const { cmd, subCmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    globalOptionsTypes: { recursive: Boolean },
    shortHands: { 'r': ['--recursive'] },
  }, ['-r', 'update'])
  t.equal(cmd, 'recursive')
  t.equal(subCmd, 'update')
  t.end()
})

test('a command is recursive if --recursive option is used', async (t) => {
  const { cmd, subCmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    globalOptionsTypes: { recursive: Boolean },
  }, ['-r', 'update'])
  t.equal(cmd, 'recursive')
  t.equal(subCmd, 'update')
  t.end()
})

test('the install command is recursive when executed in a subdir of a workspace', async (t) => {
  const { cmd, subCmd, workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    globalOptionsTypes: { dir: String },
  }, ['--dir', __dirname, 'install'])
  t.equal(cmd, 'recursive')
  t.equal(subCmd, 'install')
  t.equal(workspaceDir, path.join(__dirname, '../../..'))
  t.end()
})

test('the install command is recursive when executed in the root of a workspace', async (t) => {
  const expectedWorkspaceDir = path.join(__dirname, '../../..')
  const { cmd, subCmd, workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    globalOptionsTypes: { dir: String },
  }, ['--dir', expectedWorkspaceDir, 'install'])
  t.equal(cmd, 'recursive')
  t.equal(subCmd, 'install')
  t.equal(workspaceDir, expectedWorkspaceDir)
  t.end()
})

test('when runnning a global command inside a workspace, the workspace should be ignored', async (t) => {
  const { workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    globalOptionsTypes: { global: Boolean },
  }, ['--global', 'add', 'foo'])
  t.notOk(workspaceDir)
  t.end()
})

test('help is returned when an unknown command is used', async (t) => {
  const { cmd } = await parseCliArgs({
    ...DEFAULT_OPTS,
    globalOptionsTypes: {},
    isKnownCommand: () => false,
  }, ['foo'])
  t.equal(cmd, 'help')
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
          registry: String,
        }
      }
      return {}
    },
    globalOptionsTypes: { filter: [String, Array] },
    isKnownCommand: (commandName) => commandName === 'install',
  }, ['install', '--save-dev', '--registry=https://example.com', '--qar', '--filter=packages'])
  t.deepEqual(unknownOptions, ['save-dev', 'qar'])
  t.end()
})

test('merge option types of recursive and subcommand', async (t) => {
  const { unknownOptions } = await parseCliArgs({
    ...DEFAULT_OPTS,
    getTypesByCommandName: (commandName: string) => {
      switch (commandName) {
        case 'install': return { recursive: Boolean, registry: String }
        case 'recursive': return { sort: Boolean }
        default: return {}
      }
    },
    globalOptionsTypes: { filter: [String, Array] },
    isKnownCommand: (commandName) => commandName === 'install',
    shortHands: { 'r': ['--recursive'] },
  }, ['-r', 'install', '--registry=https://example.com', '--sort'])
  t.deepEqual(unknownOptions, [])
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
    globalOptionsTypes: {
      prefix: String,
    },
    isKnownCommand: (commandName) => commandName === 'install',
    shortHands: {
      'C': ['--prefix'],
      'r': ['--recursive'],
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
