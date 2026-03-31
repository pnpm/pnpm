import { parseCliArgs as parseCliArgsLib, type ParsedCliArgs } from '@pnpm/cli.parse-cli-args'

import {
  getCliOptionsTypes,
  getCommandFullName,
  GLOBAL_OPTIONS,
  shorthandsByCommandName,
} from './cmd/index.js'
import { shorthands as universalShorthands } from './shorthands.js'

const RENAMED_OPTIONS = {
  prefix: 'dir',
  store: 'store-dir',
}

export type ParsedCliArgsWithBuiltIn = ParsedCliArgs & { builtInCommandForced: boolean }

export async function parseCliArgs (inputArgv: string[]): Promise<ParsedCliArgsWithBuiltIn> {
  const builtInCommandForced = inputArgv[0] === 'pm'
  if (builtInCommandForced) {
    inputArgv.splice(0, 1)
  }
  const result = await parseCliArgsLib({
    fallbackCommand: 'run',
    escapeArgs: ['create', 'exec', 'test'],
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }, inputArgv)
  return { ...result, builtInCommandForced }
}
