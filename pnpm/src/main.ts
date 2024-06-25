/* eslint-disable import/first */
if (!global['pnpm__startedAt']) {
  global['pnpm__startedAt'] = Date.now()
}
import loudRejection from 'loud-rejection'
import { packageManager } from '@pnpm/cli-meta'
import { getConfig } from '@pnpm/cli-utils'
import {
  type Config,
} from '@pnpm/config'
import { executionTimeLogger, scopeLogger } from '@pnpm/core-loggers'
import { filterPackagesFromDir } from '@pnpm/filter-workspace-packages'
import { globalWarn, logger } from '@pnpm/logger'
import { type ParsedCliArgs } from '@pnpm/parse-cli-args'
import { node } from '@pnpm/plugin-commands-env'
import { finishWorkers } from '@pnpm/worker'
import chalk from 'chalk'
import { checkForUpdates } from './checkForUpdates'
import { pnpmCmds, rcOptionsTypes } from './cmd'
import { formatUnknownOptionsError } from './formatError'
import { parseCliArgs } from './parseCliArgs'
import { initReporter, type ReporterType } from './reporter'
import { isCI } from 'ci-info'
import path from 'path'
import isEmpty from 'ramda/src/isEmpty'
import stripAnsi from 'strip-ansi'
import which from 'which'

export const REPORTER_INITIALIZED = Symbol('reporterInitialized')

loudRejection()

const DEPRECATED_OPTIONS = new Set([
  'independent-leaves',
  'lock',
  'resolution-strategy',
])

// A workaround for the https://github.com/vercel/pkg/issues/897 issue.
delete process.env.PKG_EXECPATH

export async function main (inputArgv: string[]): Promise<void> {
  let parsedCliArgs!: ParsedCliArgs
  try {
    parsedCliArgs = await parseCliArgs(inputArgv)
  } catch (err: any) { // eslint-disable-line
    // Reporting is not initialized at this point, so just printing the error
    printError(err.message, err['hint'])
    process.exitCode = 1
    return
  }
  const {
    argv,
    params: cliParams,
    options: cliOptions,
    cmd,
    fallbackCommandUsed,
    unknownOptions,
    workspaceDir,
  } = parsedCliArgs
  if (cmd !== null && !pnpmCmds[cmd]) {
    printError(`Unknown command '${cmd}'`, 'For help, run: pnpm help')
    process.exitCode = 1
    return
  }

  if (unknownOptions.size > 0 && !fallbackCommandUsed) {
    const unknownOptionsArray = Array.from(unknownOptions.keys())
    if (unknownOptionsArray.every((option) => DEPRECATED_OPTIONS.has(option))) {
      let deprecationMsg = `${chalk.bgYellow.black('\u2009WARN\u2009')}`
      if (unknownOptionsArray.length === 1) {
        deprecationMsg += ` ${chalk.yellow(`Deprecated option: '${unknownOptionsArray[0]}'`)}`
      } else {
        deprecationMsg += ` ${chalk.yellow(`Deprecated options: ${unknownOptionsArray.map(unknownOption => `'${unknownOption}'`).join(', ')}`)}`
      }
      console.log(deprecationMsg)
    } else {
      printError(formatUnknownOptionsError(unknownOptions), `For help, run: pnpm help${cmd ? ` ${cmd}` : ''}`)
      process.exitCode = 1
      return
    }
  }

  let config: Config & {
    argv: { remain: string[], cooked: string[], original: string[] }
    fallbackCommandUsed: boolean
  }
  try {
    // When we just want to print the location of the global bin directory,
    // we don't need the write permission to it. Related issue: #2700
    const globalDirShouldAllowWrite = cmd !== 'root'
    const isDlxCommand = cmd === 'dlx'
    config = await getConfig(cliOptions, {
      excludeReporter: false,
      globalDirShouldAllowWrite,
      rcOptionsTypes,
      workspaceDir,
      checkUnknownSetting: false,
      ignoreNonAuthSettingsFromLocal: isDlxCommand,
    }) as typeof config
    if (isDlxCommand) {
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
    if (isCI || !process.stdout.isTTY) return 'append-only'
    return 'default'
  })()

  const printLogs = !config['parseable'] && !config['json']
  if (printLogs) {
    initReporter(reporterType, {
      cmd,
      config,
    })
    global[REPORTER_INITIALIZED] = reporterType
  }

  const selfUpdate = config.global && (cmd === 'add' || cmd === 'update') && cliParams.includes(packageManager.name)

  if (selfUpdate) {
    await pnpmCmds.server(config as any, ['stop']) // eslint-disable-line @typescript-eslint/no-explicit-any
    try {
      const currentPnpmDir = path.dirname(which.sync('pnpm'))
      if (path.relative(currentPnpmDir, config.bin) !== '') {
        console.log(`The location of the currently running pnpm differs from the location where pnpm will be installed
 Current pnpm location: ${currentPnpmDir}
 Target location: ${config.bin}
`)
      }
    } catch (err) {
      // if pnpm not found, then ignore
    }
  }

  if (
    (cmd === 'install' || cmd === 'import' || cmd === 'dedupe' || cmd === 'patch-commit' || cmd === 'patch' || cmd === 'patch-remove') &&
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
    } else if (workspaceDir && !config.includeWorkspaceRoot && (cmd === 'run' || cmd === 'exec' || cmd === 'add' || cmd === 'test')) {
      filters.push({ filter: `!{${relativeWSDirPath()}}`, followProdDepsOnly: Boolean(config.filterProd.length) })
    }

    const filterResults = await filterPackagesFromDir(wsDir, filters, {
      engineStrict: config.engineStrict,
      nodeVersion: config.nodeVersion ?? config.useNodeVersion,
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
    config.allProjectsGraph = filterResults.allProjectsGraph
    config.selectedProjectsGraph = filterResults.selectedProjectsGraph
    if (isEmpty(config.selectedProjectsGraph)) {
      if (printLogs) {
        console.log(`No projects matched the filters in "${wsDir}"`)
      }
      process.exitCode = config.failIfNoMatch ? 1 : 0
      return
    }
    if (filterResults.unmatchedFilters.length !== 0 && printLogs) {
      console.log(`No projects matched the filters "${filterResults.unmatchedFilters.join(', ')}" in "${wsDir}"`)
    }
    config.allProjects = filterResults.allProjects
    config.workspaceDir = wsDir
  }

  let { output, exitCode }: { output?: string | null, exitCode: number } = await (async () => {
    // NOTE: we defer the next stage, otherwise reporter might not catch all the logs
    await new Promise<void>((resolve) => setTimeout(() => {
      resolve()
    }, 0))

    if (
      config.updateNotifier !== false &&
      !isCI &&
      !selfUpdate &&
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
            selected: Object.keys(config.selectedProjectsGraph!).length,
            total: config.allProjects!.length,
          }
      ),
      ...(workspaceDir ? { workspacePrefix: workspaceDir } : {}),
    })

    if (config.useNodeVersion != null) {
      if ('webcontainer' in process.versions) {
        globalWarn('Automatic installation of different Node.js versions is not supported in WebContainer')
      } else {
        const nodePath = await node.getNodeBinDir(config)
        config.extraBinPaths.push(nodePath)
        config.nodeVersion = config.useNodeVersion
      }
    }
    let result = pnpmCmds[cmd ?? 'help'](
      // TypeScript doesn't currently infer that the type of config
      // is `Omit<typeof config, 'reporter'>` after the `delete config.reporter` statement
      config as Omit<typeof config, 'reporter'>,
      cliParams
    )
    if (result instanceof Promise) {
      result = await result
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
  // When use-node-version is set and "pnpm run" is executed,
  // this will be the only place where the tarball worker pool is finished.
  await finishWorkers()
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
  console.log(`${message.startsWith(ERROR) ? '' : ERROR + ' '}${chalk.red(message)}`)
  if (hint) {
    console.log(hint)
  }
}
