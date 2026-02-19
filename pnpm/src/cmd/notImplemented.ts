import { PnpmError } from '@pnpm/error'
import { type CommandDefinition } from './index.js'

const NOT_IMPLEMENTED_COMMANDS = [
  'access',
  'adduser',
  'bugs',
  'deprecate',
  'dist-tag',
  'docs',
  'edit',
  'find',
  'home',
  'info',
  'issues',
  'login',
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
  'show',
  'star',
  'stars',
  'team',
  'token',
  'unpublish',
  'unstar',
  'v',
  'version',
  'view',
  'whoami',
  'xmas',
]

export const notImplementedCommandDefinitions: CommandDefinition[] = NOT_IMPLEMENTED_COMMANDS.map(
  (commandName) => ({
    commandNames: [commandName],
    cliOptionsTypes: () => ({}),
    rcOptionsTypes: () => ({}),
    help: () => `pnpm ${commandName} is not yet implemented`,
    handler: async () => {
      throw new PnpmError('NOT_IMPLEMENTED', `The "${commandName}" command is not yet implemented in pnpm`)
    },
    skipPackageManagerCheck: true,
  })
)
