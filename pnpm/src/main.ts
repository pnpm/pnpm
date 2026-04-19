export type Global = typeof globalThis & {
  pnpm__startedAt?: number
  [REPORTER_INITIALIZED]?: ReporterType // eslint-disable-line @typescript-eslint/no-use-before-define
}
declare const global: Global
if (!global['pnpm__startedAt']) {
  global['pnpm__startedAt'] = Date.now()
}
import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { isExecutedByCorepack, packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { executionTimeLogger, scopeLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import { globalWarn, logger } from '@pnpm/logger'
import type { EngineDependency } from '@pnpm/types'
import { finishWorkers } from '@pnpm/worker'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { filterProjectsFromDir } from '@pnpm/workspace.projects-filter'
import chalk from 'chalk'
import loudRejection from 'loud-rejection'
import { isEmpty } from 'ramda'
import semver from 'semver'

import { checkForUpdates } from './checkForUpdates.js'
import { NOT_IMPLEMENTED_COMMAND_SET, overridableByScriptCommands, pnpmCmds, recursiveByDefaultCommands, skipPackageManagerCheckForCommand } from './cmd/index.js'
import { formatUnknownOptionsError } from './formatError.js'
import { getConfig, installConfigDepsAndLoadHooks } from './getConfig.js'
import type { ParsedCliArgsWithBuiltIn } from './parseCliArgs.js'
import { parseCliArgs } from './parseCliArgs.js'
import { initReporter, type ReporterType } from './reporter/index.js'
import { switchCliVersion } from './switchCliVersion.js'

export const REPORTER_INITIALIZED = Symbol('reporterInitialized')

loudRejection()

function isRootOnlyPatterns (patterns: string[]): boolean {
  return patterns.length === 1 && patterns[0] === '.'
}

// This prevents the program from crashing when the pipe's read side closes early
// (e.g., when running `pnpm config list | head`)
process.stdout.on('error', (err: NodeJS.ErrnoException) => {
  if (err.code === 'EPIPE') {
    process.exit(0)
  }
  throw err
})

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
  let {
    argv,
    params: cliParams,
    options: cliOptions,
    cmd,
    fallbackCommandUsed,
    builtInCommandForced,
    unknownOptions,
    workspaceDir,
  } = parsedCliArgs
  if (cmd !== null && !pnpmCmds[cmd]) {
    printError(`Unknown command '${cmd}'`, 'For help, run: pnpm help')
    process.exitCode = 1
    return
  }

  if (unknownOptions.size > 0 && !fallbackCommandUsed && !(cmd && NOT_IMPLEMENTED_COMMAND_SET.has(cmd))) {
    printError(formatUnknownOptionsError(unknownOptions), `For help, run: pnpm help${cmd ? ` ${cmd}` : ''}`)
    process.exitCode = 1
    return
  }

  let config: Config & {
    argv: { remain: string[], cooked: string[], original: string[] }
    fallbackCommandUsed: boolean
    parseable?: boolean
    json?: boolean
  }
  let context: ConfigContext
  try {
    // When we just want to print the location of the global bin directory,
    // we don't need the write permission to it. Related issue: #2700
    const globalDirShouldAllowWrite = cmd !== 'root'
    const isDlxOrCreateCommand = cmd === 'dlx' || cmd === 'create'
    if (cmd === 'link' && cliParams.length === 0) {
      cliOptions.global = true
    }
    ;({ config, context } = await getConfig(cliOptions, {
      excludeReporter: false,
      globalDirShouldAllowWrite,
      workspaceDir,
      onlyInheritDlxSettingsFromLocal: isDlxOrCreateCommand,
    }) as { config: typeof config, context: ConfigContext })
    if (!isExecutedByCorepack() && cmd !== 'setup' && context.wantedPackageManager != null && !shouldSkipPmHandling(cmd, cliParams)) {
      const pm = context.wantedPackageManager
      if (pm.onFail === 'download' && pm.name === 'pnpm') {
        await switchCliVersion(config, context)
      } else if (pm.onFail !== 'ignore') {
        if (cliOptions.global) {
          globalWarn('Using --global skips the package manager check for this project')
        } else {
          checkPackageManager(pm)
        }
      }
    }
    ;({ config, context } = await installConfigDepsAndLoadHooks(config, context) as { config: typeof config, context: ConfigContext })
    if (isDlxOrCreateCommand || cmd === 'sbom' || cmd === 'with') {
      config.useStderr = true
    }
    config.argv = argv
    config.fallbackCommandUsed = fallbackCommandUsed
    // Set 'npm_command' env variable to current command name
    if (cmd) {
      config.extraEnv = {
        ...config.extraEnv,
        // Follow the behavior of npm by setting it to 'run-script' when running scripts (e.g. pnpm run dev)
        // and to the command name otherwise (e.g. pnpm test)
        npm_command: cmd === 'run' ? 'run-script' : cmd,
      }
    }
  } catch (err: any) { // eslint-disable-line
    // Reporting is not initialized at this point, so just printing the error
    const hint = err['hint'] ? err['hint'] : `For help, run: pnpm help${cmd ? ` ${cmd}` : ''}`
    printError(err.message, hint)
    process.exitCode = 1
    await finishWorkers()
    return
  }
  if (cmd == null && cliOptions.version) {
    console.log(packageManager.version)
    return
  }

  let write: (text: string) => void = process.stdout.write.bind(process.stdout)
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
    write = (text) => process.stdout.write(stripAnsi(text))
  }

  const reporterType: ReporterType = (() => {
    if (config.loglevel === 'silent') return 'silent'
    if (config.reporter) return config.reporter as ReporterType
    if (config.ci || !process.stdout.isTTY) return 'append-only'
    return 'default'
  })()

  const printLogs = !config['parseable'] && !config['json']
  if (printLogs) {
    initReporter(reporterType, {
      cmd,
      config: { ...config, ...context },
    })
    global[REPORTER_INITIALIZED] = reporterType
  }

  // Commands with scriptOverride: if the current project's package.json has a
  // script with the same name, run the script instead of the built-in command.
  const typedCommandName = argv.remain[0]
  if (cmd != null && !builtInCommandForced && overridableByScriptCommands.has(typedCommandName) && !cliOptions.global) {
    const currentDirManifest = config.dir === context.rootProjectManifestDir
      ? context.rootProjectManifest
      : await safeReadProjectManifestOnly(config.dir)
    if (currentDirManifest?.scripts?.[typedCommandName]) {
      // Redirect to "pnpm run <cmd>"
      cmd = 'run'
      cliParams.unshift(typedCommandName)
      fallbackCommandUsed = true
      config.fallbackCommandUsed = true
      config.extraEnv = {
        ...config.extraEnv,
        npm_command: 'run-script',
      }
    } else if (
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
  }

  if (
    cmd != null && recursiveByDefaultCommands.has(cmd) &&
    typeof workspaceDir === 'string'
  ) {
    cliOptions['recursive'] = true
    config.recursive = true

    if (!config.recursiveInstall && !config.filter && !config.filterProd) {
      config.filter = ['{.}...']
    }
  }

  if (cliOptions['recursive']) {
    const wsDir = workspaceDir ?? process.cwd()

    config.filter = config.filter ?? []
    config.filterProd = config.filterProd ?? []

    const filters = [
      ...config.filter.map((filter) => ({ filter, followProdDepsOnly: false })),
      ...config.filterProd.map((filter) => ({ filter, followProdDepsOnly: true })),
    ]
    const relativeWSDirPath = () => path.relative(process.cwd(), wsDir) || '.'
    if (config.workspaceRoot) {
      filters.push({ filter: `{${relativeWSDirPath()}}`, followProdDepsOnly: Boolean(config.filterProd.length) })
    } else if (
      filters.length === 0 &&
      workspaceDir &&
      config.workspacePackagePatterns &&
      !isRootOnlyPatterns(config.workspacePackagePatterns) &&
      !config.includeWorkspaceRoot &&
      (cmd === 'run' || cmd === 'exec' || cmd === 'add' || cmd === 'test')
    ) {
      filters.push({ filter: `!{${relativeWSDirPath()}}`, followProdDepsOnly: Boolean(config.filterProd.length) })
    }

    const filterResults = await filterProjectsFromDir(wsDir, filters, {
      engineStrict: config.engineStrict,
      nodeVersion: config.nodeVersion,
      patterns: config.workspacePackagePatterns,
      linkWorkspacePackages: !!config.linkWorkspacePackages,
      prefix: process.cwd(),
      workspaceDir: wsDir,
      testPattern: config.testPattern,
      changedFilesIgnorePattern: config.changedFilesIgnorePattern,
      useGlobDirFiltering: !config.legacyDirFiltering,
      sharedWorkspaceLockfile: config.sharedWorkspaceLockfile,
    })

    if (filterResults.allProjects.length === 0) {
      if (printLogs) {
        console.log(`No projects found in "${wsDir}"`)
      }
      process.exitCode = config.failIfNoMatch ? 1 : 0
      return
    }
    context.allProjectsGraph = filterResults.allProjectsGraph
    context.selectedProjectsGraph = filterResults.selectedProjectsGraph
    if (isEmpty(context.selectedProjectsGraph)) {
      if (printLogs) {
        console.log(`No projects matched the filters in "${wsDir}"`)
      }
      if (config.failIfNoMatch) {
        process.exitCode = 1
        return
      }
      if (cmd !== 'list') {
        process.exitCode = 0
        return
      }
    }
    if (filterResults.unmatchedFilters.length !== 0 && printLogs) {
      console.log(`No projects matched the filters "${filterResults.unmatchedFilters.join(', ')}" in "${wsDir}"`)
    }
    context.allProjects = filterResults.allProjects
    config.workspaceDir = wsDir
  }

  let { output, exitCode }: { output?: string | null, exitCode: number } = await (async () => {
    // NOTE: we defer the next stage, otherwise reporter might not catch all the logs
    await new Promise<void>((resolve) => setTimeout(() => {
      resolve()
    }, 0))

    if (
      config.updateNotifier !== false &&
      !config.ci &&
      cmd !== 'self-update' &&
      !config.offline &&
      !config.preferOffline &&
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

    scopeLogger.debug({
      ...(
        !cliOptions['recursive']
          ? { selected: 1 }
          : {
            selected: Object.keys(context.selectedProjectsGraph!).length,
            total: context.allProjects!.length,
          }
      ),
      ...(workspaceDir ? { workspacePrefix: workspaceDir } : {}),
    })
    let result = pnpmCmds[cmd ?? 'help'](
      // Spread config (settings) and context (runtime state) into a single
      // options object for command handlers. The original split objects are
      // also passed for handlers that need them separated (e.g. config commands).
      // Named "_config"/"_context" to avoid clashing with the "--config" CLI option.
      { ...config, ...context, _config: config, _context: context } as Omit<typeof config & ConfigContext, 'reporter'>,
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
    if (!result) {
      return { output: null, exitCode: 0 }
    }
    if (typeof result === 'string') {
      return { output: result, exitCode: 0 }
    }
    return result
  })()
  if (output) {
    if (!output.endsWith('\n')) {
      output = `${output}\n`
    }
    write(output)
  }
  if (!cmd) {
    exitCode = 1
  }
  if (exitCode) {
    process.exitCode = exitCode
  }
}

function printError (message: string, hint?: string): void {
  const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')
  console.error(`${message.startsWith(ERROR) ? '' : ERROR + ' '}${chalk.red(message)}`)
  if (hint) {
    console.error(hint)
  }
}

/**
 * Whether to skip the packageManager/devEngines handling block (both auto
 * download and warn/error check). Returns true when the command itself
 * opts out via `skipPackageManagerCheck: true`, or when the user is asking
 * for help on such a command — `pnpm help <skippable>` and
 * `pnpm <skippable> --help` (which parse-cli-args rewrites to the same
 * cmd='help' form) shouldn't download an older pinned pnpm just to render
 * help for a command that older pnpm may not even have.
 */
function shouldSkipPmHandling (cmd: string | null, cliParams: string[]): boolean {
  if (cmd == null) return false
  if (skipPackageManagerCheckForCommand.has(cmd)) return true
  if (cmd === 'help' && cliParams[0] != null && skipPackageManagerCheckForCommand.has(cliParams[0])) return true
  return false
}

function checkPackageManager (pm: EngineDependency): void {
  if (!pm.name) return
  const shouldError = pm.onFail === 'error' || pm.onFail === 'download'
  if (pm.name !== 'pnpm') {
    const msg = `This project is configured to use ${pm.name}`
    if (shouldError) {
      throw new PnpmError('OTHER_PM_EXPECTED', msg)
    }
    globalWarn(msg)
  } else if (pm.version) {
    const currentPnpmVersion = packageManager.name === 'pnpm'
      ? packageManager.version
      : undefined
    if (currentPnpmVersion && !semver.satisfies(currentPnpmVersion, pm.version, { includePrerelease: true })) {
      const msg = `This project is configured to use ${pm.version} of pnpm. Your current pnpm is v${currentPnpmVersion}`
      if (shouldError) {
        throw new PnpmError('BAD_PM_VERSION', msg, {
          hint: 'If you want to bypass this version check, you can set the "pmOnFail" configuration to "warn" or "ignore" (e.g. via --pm-on-fail=ignore). If using "devEngines.packageManager", you can set its "onFail" to "warn" or "ignore"',
        })
      } else {
        globalWarn(msg)
      }
    }
  }
}
