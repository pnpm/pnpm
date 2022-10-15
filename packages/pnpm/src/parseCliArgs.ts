import { parseCliArgs as parseCliArgsLib } from '@pnpm/parse-cli-args'
import {
  getCliOptionsTypes,
  getCommandFullName,
  GLOBAL_OPTIONS,
  shorthandsByCommandName,
} from './cmd'
import { shorthands as universalShorthands } from './shorthands'

const RENAMED_OPTIONS = {
  'lockfile-directory': 'lockfile-dir',
  prefix: 'dir',
  'shrinkwrap-directory': 'lockfile-dir',
  store: 'store-dir',
}

export async function parseCliArgs (inputArgv: string[]) {
  return parseCliArgsLib({
    fallbackCommand: 'run',
    escapeArgs: ['create', 'dlx', 'exec'],
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }, inputArgv)
}
