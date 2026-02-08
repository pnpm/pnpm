import { type ParsedCliArgs, parseCliArgs as parseCliArgsLib } from '@pnpm/parse-cli-args'
import {
  getCliOptionsTypes,
  getCommandFullName,
  GLOBAL_OPTIONS,
  shorthandsByCommandName,
  subcommandsByCommandName,
} from './cmd/index.js'
import { shorthands as universalShorthands } from './shorthands.js'

const RENAMED_OPTIONS = {
  'lockfile-directory': 'lockfile-dir',
  prefix: 'dir',
  'shrinkwrap-directory': 'lockfile-dir',
  store: 'store-dir',
}

export async function parseCliArgs (inputArgv: string[]): Promise<ParsedCliArgs> {
  return parseCliArgsLib({
    fallbackCommand: 'run',
    escapeArgs: ['create', 'exec', 'test'],
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    subcommandsByCommandName,
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }, inputArgv)
}
