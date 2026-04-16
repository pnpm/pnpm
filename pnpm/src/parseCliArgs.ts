import { parseCliArgs as parseCliArgsLib, type ParsedCliArgs } from '@pnpm/cli.parse-cli-args'
import { PnpmError } from '@pnpm/error'

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
  let result = await parseCliArgsLib({
    fallbackCommand: 'run',
    escapeArgs: ['create', 'exec', 'test'],
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }, inputArgv)
  // `pnpm with current <cmd> [args]` is sugar for
  // `pnpm --config.package-manager-on-fail=ignore <cmd> [args]` —
  // re-parse so the inner command is dispatched directly, in-process.
  if (result.cmd === 'with' && result.params[0] === 'current') {
    const rest = result.params.slice(1)
    if (rest.length === 0) {
      throw new PnpmError('MISSING_WITH_CURRENT_CMD',
        'Missing command after "current". Usage: pnpm with current <command> [args...]')
    }
    result = await parseCliArgsLib({
      fallbackCommand: 'run',
      escapeArgs: ['create', 'exec', 'test'],
      getCommandLongName: getCommandFullName,
      getTypesByCommandName: getCliOptionsTypes,
      renamedOptions: RENAMED_OPTIONS,
      shorthandsByCommandName,
      universalOptionsTypes: GLOBAL_OPTIONS,
      universalShorthands,
    }, ['--config.package-manager-on-fail=ignore', ...rest])
  }
  return { ...result, builtInCommandForced }
}
