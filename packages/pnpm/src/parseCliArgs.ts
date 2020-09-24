import parseCliArgsLib from '@pnpm/parse-cli-args'
import {
  getCliOptionsTypes,
  getCommandFullName,
  GLOBAL_OPTIONS,
  shorthandsByCommandName,
} from './cmd'
import universalShorthands from './shorthands'

const RENAMED_OPTIONS = {
  'lockfile-directory': 'lockfile-dir',
  prefix: 'dir',
  'shrinkwrap-directory': 'lockfile-dir',
  store: 'store-dir',
}

export default function parseCliArgs (inputArgv: string[]) {
  return parseCliArgsLib({
    fallbackCommand: 'run',
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }, inputArgv)
}
