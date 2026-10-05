import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { executionTimeLogger, scopeLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import { finishWorkers } from '@pnpm/worker'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import chalk from 'chalk'
import loudRejection from 'loud-rejection'

import { checkForUpdates } from './checkForUpdates.js'
import { checkSudo } from './checkSudo.js'
import { type Command, NOT_IMPLEMENTED_COMMAND_SET, overridableByScriptCommands, pnpmCmds } from './cmd/index.js'
import { formatUnknownOptionsError } from './formatError.js'
import { getConfig, installConfigDepsAndLoadHooks, isSingleSettingRead } from './getConfig.js'
import { handlePackageManagerAndRuntimes, shouldSkipPmHandling } from './packageManagerCheck.js'
import type { ParsedCliArgsWithBuiltIn } from './parseCliArgs.js'
import { parseCliArgs } from './parseCliArgs.js'
import { initReporter, type ReporterType } from './reporter/index.js'
import { type CommandInvocation, enableRecursiveByDefault, selectWorkspaceProjects } from './selectWorkspaceProjects.js'

export const REPORTER_INITIALIZED = Symbol('reporterInitialized')

export type Global = typeof globalThis & {
  pnpm__startedAt?: number
  [REPORTER_INITIALIZED]?: ReporterType
}
declare const global: Global
if (!global['pnpm__startedAt']) {
  global['pnpm__startedAt'] = Date.now()
}

// Commands whose reporter output (warnings, progress) must go to stderr so that
// their stdout stays a clean, machine-readable value. For example, `pnpm store
// path` is meant to be captured with `STORE=$(pnpm store path)` and `pnpm config
// list --json` to be piped into `jq`; a warning mixed into stdout would corrupt
// both.
const COMMANDS_WITH_STDERR_REPORTER = new Set(['dlx', 'create', 'config', 'set', 'get', 'sbom', 'with', 'store', 'prefix', 'root', 'bin'])

loudRejection()

// This prevents the program from crashing when the pipe's read side closes early
// (e.g., when running `pnpm config list | head`)
process.stdout.on('error', (err: NodeJS.ErrnoException) => {
  if (err.code === 'EPIPE') {
    // eslint-disable-next-line n/no-process-exit -- the reader is gone, so there is nothing left to print to
    process.exit(0)
  }
  throw err
})

type CommandConfig = Config & {
  argv: { remain: string[], cooked: string[], original: string[] }
  fallbackCommandUsed: boolean
  parseable?: boolean
  json?: boolean
}

interface LoadedConfig {
  config: CommandConfig
  context: ConfigContext
}

interface CommandResult {
  output?: string | null
  exitCode: number
}

export async function main (inputArgv: string[]): Promise<void> {
  let parsedCliArgs!: ParsedCliArgsWithBuiltIn
  try {
    parsedCliArgs = await parseCliArgs(inputArgv)
  } catch (err: any) { // eslint-disable-line
    // Reporting is not initialized at this point, so just printing the error
    printError(err.message, err['hint'])
    process.exitCode = 1
    return
  }
  const { cmd } = parsedCliArgs
  if (cmd !== null && !pnpmCmds[cmd]) {
    printError(`Unknown command '${cmd}'`, 'For help, run: pnpm help')
    process.exitCode = 1
    return
  }

  const rejectUnknownOptions = dropUnknownOptions(parsedCliArgs)
  const loaded = await loadCommandConfig(parsedCliArgs, rejectUnknownOptions)
  if (loaded == null) return
  if (cmd == null && parsedCliArgs.options.version) {
    console.log(packageManager.version)
    return
  }
  await runCommandWithConfig(parsedCliArgs, loaded)
}

/**
 * The pnpm the project pins may know an option this one does not, so the
 * unknown options are reported only once no switch to that pnpm happened.
 * Until then they are dropped from the CLI options. Returns whether they are
 * to be rejected.
 */
function dropUnknownOptions (parsedCliArgs: ParsedCliArgsWithBuiltIn): boolean {
  const { cmd, options: cliOptions, unknownOptions } = parsedCliArgs
  const rejectUnknownOptions = unknownOptions.size > 0 && !(cmd && NOT_IMPLEMENTED_COMMAND_SET.has(cmd))
  if (rejectUnknownOptions) {
    for (const unknownOption of unknownOptions.keys()) {
      delete cliOptions[unknownOption]
    }
  }
  return rejectUnknownOptions
}

/**
 * Reads the config for the command and enforces the pinned package manager
 * and runtimes. Returns `undefined`, having printed the error and set
 * `process.exitCode`, when the command cannot run.
 */
async function loadCommandConfig (
  parsedCliArgs: ParsedCliArgsWithBuiltIn,
  rejectUnknownOptions: boolean
): Promise<LoadedConfig | undefined> {
  const { cmd } = parsedCliArgs
  try {
    return await readCommandConfig(parsedCliArgs, rejectUnknownOptions)
  } catch (err: any) { // eslint-disable-line
    // Reporting is not initialized at this point, so just printing the error
    const hint = err['hint'] ? err['hint'] : `For help, run: pnpm help${cmd ? ` ${cmd}` : ''}`
    printError(err.message, hint)
    process.exitCode = 1
    await finishWorkers()
    return undefined
  }
}

async function readCommandConfig (
  parsedCliArgs: ParsedCliArgsWithBuiltIn,
  rejectUnknownOptions: boolean
): Promise<LoadedConfig | undefined> {
  const { cmd, params: cliParams, options: cliOptions, unknownOptions, workspaceDir, rawCliConfig } = parsedCliArgs
  const isConfigCommand = cmd === 'config' || cmd === 'set' || cmd === 'get'
  applyGlobalScope(parsedCliArgs, isConfigCommand)
  let { config, context } = await getConfig(cliOptions, {
    excludeReporter: false,
    // When we just want to print the location of the global bin directory,
    // we don't need the write permission to it. Related issue: pnpm/pnpm#2700
    globalDirShouldAllowWrite: cmd !== 'root' && cmd !== 'prefix',
    skipGlobalBinDirCheck: envSubcommandSkipsGlobalBinCheck(cmd, cliParams),
    workspaceDir,
    rawCliConfig,
    onlyInheritDlxSettingsFromLocal: cmd === 'dlx' || cmd === 'create',
    forSelfUpdate: cmd === 'self-update',
    ignoreProjectNpmrc: isGlobalConfigCommand(isConfigCommand, cliOptions),
    printWarnings: !isSingleSettingRead(cmd, cliParams),
  }) as LoadedConfig
  if (cmd !== 'setup' && !shouldSkipPmHandling(cmd, cliParams, cliOptions.location)) {
    await handlePackageManagerAndRuntimes({ cmd, cliOptions, config, context, rejectUnknownOptions })
  }
  if (rejectUnknownOptions) {
    printError(formatUnknownOptionsError(unknownOptions), `For help, run: pnpm help${cmd ? ` ${cmd}` : ''}`)
    process.exitCode = 1
    await finishWorkers()
    return undefined
  }
  // `pnpm set` / `pnpm get` are separate top-level commands whose handlers
  // delegate to the `config` command internally. They are not rewritten to
  // `cmd === 'config'` at this layer, so list them explicitly — users can
  // hit the pnpm/pnpm#10684 crash via any of these three entry points.
  ;({ config, context } = await installConfigDepsAndLoadHooks(config, context, {
    tolerateConfigDependenciesErrors: isConfigCommand,
    forSelfUpdate: cmd === 'self-update',
  }) as LoadedConfig)
  applyInvocationToConfig(config, parsedCliArgs)
  return { config, context }
}

/**
 * `pnpm link` without arguments links into the global directory.
 *
 * `--global` on a config command is the same request as `--location=global`.
 * It names the global config files and does not load the config of a global
 * install.
 */
function applyGlobalScope ({ cmd, params, options }: ParsedCliArgsWithBuiltIn, isConfigCommand: boolean): void {
  if (cmd === 'link' && params.length === 0) {
    options.global = true
  } else if (isConfigCommand && options.global === true) {
    options.location ??= 'global'
    delete options.global
  }
}

function isGlobalConfigCommand (isConfigCommand: boolean, cliOptions: ParsedCliArgsWithBuiltIn['options']): boolean {
  return isConfigCommand && cliOptions.location === 'global'
}

function applyInvocationToConfig (config: CommandConfig, parsedCliArgs: ParsedCliArgsWithBuiltIn): void {
  const { cmd } = parsedCliArgs
  if (cmd != null && COMMANDS_WITH_STDERR_REPORTER.has(cmd)) {
    config.useStderr = true
  }
  config.argv = parsedCliArgs.argv
  config.fallbackCommandUsed = parsedCliArgs.fallbackCommandUsed
  if (cmd) {
    config.extraEnv = {
      ...config.extraEnv,
      // Follow the behavior of npm by setting it to 'run-script' when running scripts (e.g. pnpm run dev)
      // and to the command name otherwise (e.g. pnpm test)
      npm_command: cmd === 'run' ? 'run-script' : cmd,
    }
  }
}

async function runCommandWithConfig (parsedCliArgs: ParsedCliArgsWithBuiltIn, { config, context }: LoadedConfig): Promise<void> {
  const { options: cliOptions, params: cliParams, workspaceDir } = parsedCliArgs
  const write = createOutputWriter(config)

  const printLogs = !config['parseable'] && !config['json']
  if (printLogs) {
    initCommandReporter(parsedCliArgs, { config, context })
  }

  checkSudo({ cmd: parsedCliArgs.cmd, cliParams, global: cliOptions.global, location: cliOptions.location, printLogs })

  const cmd = await redirectToOverridingScript(parsedCliArgs, { config, context })
  const invocation: CommandInvocation = { cmd, cliParams, cliOptions, workspaceDir }
  enableRecursiveByDefault(invocation, config)
  if (cliOptions['recursive'] && !await selectWorkspaceProjects(invocation, { config, context, printLogs })) {
    return
  }

  const { output, exitCode } = await runCommand(invocation, { config, context })
  if (output) {
    write(output.endsWith('\n') ? output : `${output}\n`)
  }
  const finalExitCode = cmd ? exitCode : 1
  if (finalExitCode) {
    process.exitCode = finalExitCode
  }
}

function createOutputWriter (config: CommandConfig): (text: string) => void {
  // chalk reads the FORCE_COLOR env variable
  if (config.color === 'always') {
    process.env['FORCE_COLOR'] = '1'
  } else if (config.color === 'never') {
    process.env['FORCE_COLOR'] = '0'

    // In some cases, it is already late to set the FORCE_COLOR env variable.
    // Some text might be already generated.
    //
    // A better solution might be to dynamically load all the code after the settings are read
    // and the env variable set.
    return (text) => process.stdout.write(stripAnsi(text))
  }
  return process.stdout.write.bind(process.stdout)
}

function initCommandReporter (parsedCliArgs: ParsedCliArgsWithBuiltIn, { config, context }: LoadedConfig): void {
  const { cmd, options: cliOptions, params: cliParams } = parsedCliArgs
  const reporterType = getReporterType(config)
  // `pnpm add -g` may install several isolated groups in one run, one
  // per CLI param. When that happens, force the reporter to show the
  // per-prefix progress/stats output so every group's stats line up
  // with the right install dir instead of being silently dropped.
  const multiGroupGlobalAdd = cmd === 'add' && cliOptions.global === true && cliParams.length > 1
  initReporter(reporterType, {
    cmd,
    config: { ...config, ...context },
    hideProgressPrefix: multiGroupGlobalAdd ? false : undefined,
  })
  global[REPORTER_INITIALIZED] = reporterType
}

function getReporterType (config: CommandConfig): ReporterType {
  if (config.loglevel === 'silent') return 'silent'
  if (config.reporter) return config.reporter as ReporterType
  if (config.ci || !process.stdout.isTTY) return 'append-only'
  return 'default'
}

/**
 * Commands with scriptOverride: if the current project's package.json has a
 * script with the same name, run the script instead of the built-in command.
 * Returns the command to run.
 */
async function redirectToOverridingScript (
  parsedCliArgs: ParsedCliArgsWithBuiltIn,
  { config, context }: LoadedConfig
): Promise<string | null> {
  const { cmd, params: cliParams, workspaceDir } = parsedCliArgs
  const typedCommandName = parsedCliArgs.argv.remain[0]
  if (
    cmd == null ||
    parsedCliArgs.builtInCommandForced ||
    !overridableByScriptCommands.has(typedCommandName) ||
    parsedCliArgs.options.global
  ) return cmd
  const currentDirManifest = config.dir === context.rootProjectManifestDir
    ? context.rootProjectManifest
    : await safeReadProjectManifestOnly(config.dir)
  if (currentDirManifest?.scripts?.[typedCommandName]) {
    // Redirect to "pnpm run <cmd>"
    cliParams.unshift(typedCommandName)
    config.fallbackCommandUsed = true
    config.extraEnv = {
      ...config.extraEnv,
      npm_command: 'run-script',
    }
    return 'run'
  }
  if (
    workspaceDir &&
    config.dir !== context.rootProjectManifestDir &&
    context.rootProjectManifest?.scripts?.[typedCommandName]
  ) {
    throw new PnpmError(
      'SCRIPT_OVERRIDE_IN_WORKSPACE_ROOT',
      `The workspace root has a "${typedCommandName}" script, ` +
      `so the built-in "pnpm ${typedCommandName}" command cannot run from a subdirectory`,
      {
        hint: `Run "pnpm run ${typedCommandName}" from the workspace root to execute the script`,
      }
    )
  }
  return cmd
}

async function runCommand (invocation: CommandInvocation, { config, context }: LoadedConfig): Promise<CommandResult> {
  const { cmd, cliParams } = invocation
  // NOTE: we defer the next stage, otherwise reporter might not catch all the logs
  await new Promise<void>((resolve) => setTimeout(() => {
    resolve()
  }, 0))

  warnBeforeCommand(cmd, config)

  scopeLogger.debug({
    ...(
      !invocation.cliOptions['recursive']
        ? { selected: 1 }
        : {
          selected: Object.keys(context.selectedProjectsGraph!).length,
          total: context.allProjects!.length,
        }
    ),
    ...(invocation.workspaceDir ? { workspacePrefix: invocation.workspaceDir } : {}),
  })
  let result = pnpmCmds[cmd ?? 'help'](
    // Spread config (settings) and context (runtime state) into a single
    // options object for command handlers. The original split objects are
    // also passed for handlers that need them separated (e.g. config commands).
    // Named "_config"/"_context" to avoid clashing with the "--config" CLI option.
    { ...config, ...context, _config: config, _context: context } as Omit<CommandConfig & ConfigContext, 'reporter'>,
    cliParams,
    pnpmCmds
  )
  try {
    if (result instanceof Promise) {
      result = await result
    }
  } finally {
    await finishWorkers()
  }
  executionTimeLogger.debug({
    startedAt: global['pnpm__startedAt'],
    endedAt: Date.now(),
  })
  return toCommandResult(result)
}

function warnBeforeCommand (cmd: string | null, config: CommandConfig): void {
  const updateCheckEnabled = config.updateNotifier !== false && !config.ci && !config.offline && !config.preferOffline
  if (
    updateCheckEnabled &&
    cmd !== 'self-update' &&
    !config.fallbackCommandUsed &&
    (cmd === 'install' || cmd === 'add')
  ) {
    checkForUpdates(config).catch(() => { /* Ignore */ })
  }

  if (config.force === true && !config.fallbackCommandUsed) {
    logger.warn({
      message: 'using --force I sure hope you know what you are doing',
      prefix: config.dir,
    })
  }
}

function toCommandResult (result: Awaited<ReturnType<Command>>): CommandResult {
  if (!result) {
    return { output: null, exitCode: 0 }
  }
  if (typeof result === 'string') {
    return { output: result, exitCode: 0 }
  }
  return result
}

function printError (message: string, hint?: string): void {
  const ERROR = chalk.bgRed.red('[') + chalk.bgRed.black('ERROR') + chalk.bgRed.red(']')
  console.error(`${message.startsWith(ERROR) ? '' : ERROR + ' '}${chalk.red(message)}`)
  if (hint) {
    console.error(hint)
  }
}

/**
 * `env remove` and `env list` do not link a Node.js executable into the global
 * bin directory. They have to run when that directory is absent from `PATH`,
 * which is how pnpm looks when another tool installed it.
 */
function envSubcommandSkipsGlobalBinCheck (cmd: string | null, cliParams: string[]): boolean {
  if (cmd !== 'env') return false
  switch (cliParams[0]) {
    case 'remove':
    case 'rm':
    case 'uninstall':
    case 'un':
    case 'list':
    case 'ls':
      return true
    default:
      return false
  }
}
