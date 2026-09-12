import { formatWarn } from '@pnpm/cli.default-reporter'
import { globalWarn } from '@pnpm/logger'

export interface CheckSudoOptions {
  cmd: string | null
  cliParams: string[]
  global?: boolean
  location?: string
  env?: NodeJS.ProcessEnv
  geteuid?: () => number
  /**
   * `false` when no reporter is subscribed to the log stream (`--json`,
   * `--parseable`), in which case the warning goes straight to stderr rather
   * than into a stream nobody reads. stdout stays machine-parseable either way.
   */
  printLogs?: boolean
}

export function checkSudo (opts: CheckSudoOptions): void {
  const operation = sudoBlockedOperation(opts)
  if (operation == null) return
  const message = `Running "${operation}" with sudo is not supported and will fail in pnpm v12. pnpm installs global packages and writes global configuration inside your home directory, so they do not require root permissions, and running this command as root targets the root user's home directory instead of yours. Rerun the command without sudo. If you really intend to manage the root user's own global packages, run pnpm from a session where the SUDO_USER environment variable is not set (for example: sudo env -u SUDO_USER pnpm ...).`
  if (opts.printLogs === false) {
    console.warn(formatWarn(message))
  } else {
    globalWarn(message)
  }
}

const READ_ONLY_GLOBAL_COMMANDS = new Set([
  'audit', 'bin', 'get', 'la', 'licenses', 'list', 'll', 'ls', 'outdated', 'prefix', 'root', 'why',
])

/**
 * The user-facing name of the operation that sudo makes meaningless, or
 * `undefined` when the command is fine under sudo. Global commands that only
 * read (`pnpm bin -g`, `pnpm list -g`, `pnpm config get -g`, ...) stay allowed.
 */
export function sudoBlockedOperation (opts: CheckSudoOptions): string | undefined {
  const env = opts.env ?? process.env
  const geteuid = opts.geteuid ?? process.geteuid?.bind(process)
  if (geteuid == null || geteuid() !== 0) return undefined
  if (!env.SUDO_USER || env.SUDO_USER === 'root') return undefined
  const { cmd, cliParams, global: globalFlag, location } = opts
  if (cmd === 'setup' || cmd === 'self-update') return `pnpm ${cmd}`
  if (cmd === 'config' || cmd === 'set') {
    const subcommand = cmd === 'set' ? 'set' : cliParams[0]
    if (subcommand !== 'set' && subcommand !== 'delete') return undefined
    // Config writes default to the global config file when no `--location`
    // is given, so gate on the effective scope, not the `--global` flag
    // alone. Mirrors the scope resolution in the config command handler.
    const effectiveGlobal = location != null ? location === 'global' : globalFlag !== false
    return effectiveGlobal ? `pnpm config ${subcommand} --global` : undefined
  }
  if (globalFlag !== true || cmd == null) return undefined
  if (READ_ONLY_GLOBAL_COMMANDS.has(cmd)) return undefined
  return `pnpm ${cmd} --global`
}
