import parseCliArgs from '@pnpm/parse-cli-args'
import os = require('os')
import path = require('path')
import test = require('tape')

const DEFAULT_OPTS = {
  getCommandLongName: (commandName: string) => commandName,
  getTypesByCommandName: (commandName: string) => ({}),
  renamedOptions: { 'prefix': 'dir' },
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
    universalShorthands: { 'r': '--recursive' },
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

test('the install command is recursive when executed in a subdir of a workspace', async (t) => {
  const { options, cmd, workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { dir: String },
  }, ['--dir', __dirname, 'install'])
  t.equal(cmd, 'install')
  t.ok(options['recursive'])
  t.equal(workspaceDir, path.join(__dirname, '../../..'))
  t.end()
})

test('the install command is recursive when executed in the root of a workspace', async (t) => {
  const expectedWorkspaceDir = path.join(__dirname, '../../..')
  const { options, cmd, workspaceDir } = await parseCliArgs({
    ...DEFAULT_OPTS,
    universalOptionsTypes: { dir: String },
  }, ['--dir', expectedWorkspaceDir, 'install'])
  t.equal(cmd, 'install')
  t.ok(options['recursive'])
  t.equal(workspaceDir, expectedWorkspaceDir)
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
  }, ['recursive', 'foo'])
  t.equal(cmd, 'foo')
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
          recursive: Boolean,
          registry: String,
        }
      }
      return {}
    },
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
          'dev': Boolean,
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
