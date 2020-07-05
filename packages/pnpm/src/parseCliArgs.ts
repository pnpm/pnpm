import parseCliArgsLib from '@pnpm/parse-cli-args'
import pnpmCmds, {
  getCliOptionsTypes,
  getCommandFullName,
  GLOBAL_OPTIONS,
  shorthandsByCommandName,
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
    knownCommands: new Set(Object.keys(pnpmCmds)),
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }, inputArgv)
}
