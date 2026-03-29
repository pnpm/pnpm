import { PnpmError } from '@pnpm/error'

import type { CommandDefinition } from './index.js'

const NOT_IMPLEMENTED_COMMANDS = [
  'access',
  'bugs',
  'dist-tag',
  'docs',
  'edit',
  'find',
  'home',
  'issues',
  'logout',
  'owner',
  'ping',
  'prefix',
  'profile',
  'pkg',
  'repo',
  's',
  'se',
  'search',
  'set-script',
  'star',
  'stars',
  'team',
  'token',
  'unpublish',
  'unstar',
  'whoami',
  'xmas',
]

export const NOT_IMPLEMENTED_COMMAND_SET = new Set(NOT_IMPLEMENTED_COMMANDS)

export const notImplementedCommandDefinitions: CommandDefinition[] = NOT_IMPLEMENTED_COMMANDS.map(
  (commandName) => ({
    commandNames: [commandName],
    cliOptionsTypes: () => ({}),
    rcOptionsTypes: () => ({}),
    help: () => `pnpm ${commandName} is not yet implemented`,
    handler: async () => {
      throw new PnpmError('NOT_IMPLEMENTED', `The "${commandName}" command is not yet implemented in pnpm. Use the npm CLI directly: npm ${commandName}`)
    },
    skipPackageManagerCheck: true,
  })
)
