import { globalWarn } from '@pnpm/logger'

export interface CheckSudoOptions {
  cmd: string | null
  cliParams: string[]
  global?: boolean
  location?: string
  env?: NodeJS.ProcessEnv
  geteuid?: () => number
}

/**
 * Warns about commands that write to home-directory locations (global
 * installs, global config writes, `pnpm setup`, `pnpm self-update`) when pnpm
 * is executed through `sudo`. Those commands target root's home directory,
 * which is never what a user coming from `sudo npm install -g` wants.
 *
 * pnpm v12 refuses to run them; v11 only warns, so setups that rely on the
 * current behavior keep working until the next major.
 */
export function checkSudo (opts: CheckSudoOptions): void {
  const operation = sudoBlockedOperation(opts)
  if (operation == null) return
  globalWarn(`Running "${operation}" with sudo is not supported and will fail in pnpm v12. pnpm installs global packages and writes global configuration inside your home directory, so they do not require root permissions, and running this command as root targets the root user's home directory instead of yours. Rerun the command without sudo. If you really intend to manage the root user's own global packages, run pnpm from a session where the SUDO_USER environment variable is not set (for example: sudo env -u SUDO_USER pnpm ...).`)
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
