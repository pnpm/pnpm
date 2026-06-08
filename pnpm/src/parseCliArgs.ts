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
  const libOpts = {
    fallbackCommand: 'run',
    escapeArgs: ['create', 'exec', 'test'],
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }
  let result = await parseCliArgsLib(libOpts, inputArgv)
  // `pnpm [global-opts] with current <cmd> [args]` is sugar for
  // `pnpm [global-opts] --pm-on-fail=ignore <cmd> [args]` — re-parse so the
  // inner command is dispatched directly, in-process. The override is
  // propagated via env var (not --pm-on-fail=ignore in argv) so it survives
  // parseCliArgsLib's special short-circuits like the -v/--version
  // interceptor, which discards other parsed options.
  //
  // We rebuild argv by removing the `with current` tokens in place so that
  // any global flags the user put BEFORE `with` (e.g. `--dir`, `--filter`)
  // are preserved.
  if (result.cmd === 'with' && result.params[0] === 'current') {
    const withIdx = findWithCurrentIndex(inputArgv, GLOBAL_OPTIONS)
    if (withIdx < 0 || inputArgv.length - withIdx - 2 === 0) {
      throw new PnpmError('MISSING_WITH_CURRENT_CMD',
        'Missing command after "current". Usage: pnpm with current <command> [args...]')
    }
    process.env.pnpm_config_pm_on_fail = 'ignore'
    result = await parseCliArgsLib(libOpts, [
      ...inputArgv.slice(0, withIdx),
      ...inputArgv.slice(withIdx + 2),
    ])
  }
  return { ...result, builtInCommandForced }
}

/**
 * Locate the `with current` token pair in argv. We assume the first
 * occurrence that's plausibly the command (not the value of a preceding flag)
 * is the one. Good enough for realistic CLI usage — no pnpm option is
 * expected to take the literal value `with`.
 */
function findWithCurrentIndex (argv: string[], optionTypes: Record<string, unknown>): number {
  for (let i = 0; i < argv.length - 1; i++) {
    if (argv[i] !== 'with' || argv[i + 1] !== 'current') continue
    const prev = argv[i - 1]
    // A preceding long option that takes a value would consume `with` as its
    // value, so this `with current` pair isn't the command — skip it. Boolean
    // flags (e.g. `--color`) and `--no-` negations don't consume a value.
    if (prev != null && longOptionConsumesValue(prev, optionTypes)) continue
    return i
  }
  return -1
}

function longOptionConsumesValue (token: string, optionTypes: Record<string, unknown>): boolean {
  if (!token.startsWith('--') || token.includes('=')) return false
  const name = token.slice(2)
  if (name.startsWith('no-')) return false
  return !isBooleanType(optionTypes[name])
}

function isBooleanType (type: unknown): boolean {
  return type === Boolean || (Array.isArray(type) && type.includes(Boolean))
}
