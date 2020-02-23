import parseCliArgsLib from '@pnpm/parse-cli-args'
import pnpmCmds, {
  getCliOptionsTypes,
  getCommandFullName,
  GLOBAL_OPTIONS,
} from './cmd'
import universalShorthands from './shorthands'

const RENAMED_OPTIONS = {
  'lockfile-directory': 'lockfile-dir',
  'prefix': 'dir',
  'shrinkwrap-directory': 'lockfile-dir',
  'store': 'store-dir',
}

export default function parseCliArgs (inputArgv: string[]) {
  return parseCliArgsLib({
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    isKnownCommand: (commandName) => typeof pnpmCmds[commandName] !== 'undefined',
    renamedOptions: RENAMED_OPTIONS,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }, inputArgv)
}
