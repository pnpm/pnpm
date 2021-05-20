// Map SIGINT & SIGTERM to process exit
// so that lockfiles are removed automatically
import loudRejection from 'loud-rejection'
import packageManager from '@pnpm/cli-meta'
import { getConfig } from '@pnpm/cli-utils'
import {
  Config,
} from '@pnpm/config'
import { scopeLogger } from '@pnpm/core-loggers'
import { filterPackages } from '@pnpm/filter-workspace-packages'
import findWorkspacePackages from '@pnpm/find-workspace-packages'
import logger from '@pnpm/logger'
import { ParsedCliArgs } from '@pnpm/parse-cli-args'
import { node } from '@pnpm/plugin-commands-nvm'
import chalk from 'chalk'
import checkForUpdates from './checkForUpdates'
import pnpmCmds, { rcOptionsTypes } from './cmd'
import { formatUnknownOptionsError } from './formatError'
import './logging/fileLogger'
import parseCliArgs from './parseCliArgs'
import initReporter, { ReporterType } from './reporter'
import isCI from 'is-ci'
import path from 'path'
import * as R from 'ramda'
import stripAnsi from 'strip-ansi'
import which from 'which'

process
  .once('SIGINT', () => process.exit(0))
  .once('SIGTERM', () => process.exit(0))

loudRejection()

const DEPRECATED_OPTIONS = new Set([
  'independent-leaves',
  'lock',
  'resolution-strategy',
])

// A workaround for the https://github.com/vercel/pkg/issues/897 issue.
delete process.env.PKG_EXECPATH

export default async function run (inputArgv: string[]) {
  let parsedCliArgs!: ParsedCliArgs
  try {
    parsedCliArgs = await parseCliArgs(inputArgv)
  } catch (err) {
    // Reporting is not initialized at this point, so just printing the error
    printError(err.message, err['hint'])
    process.exit(1)
  }
  const {
    argv,
    params: cliParams,
    options: cliOptions,
    cmd,
    unknownOptions,
    workspaceDir,
  } = parsedCliArgs
  if (cmd !== null && !pnpmCmds[cmd]) {
    printError(`Unknown command '${cmd}'`, 'For help, run: pnpm help')
    process.exit(1)
  }

  if (unknownOptions.size > 0 && cmd !== 'node') {
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
      process.exit(1)
    }
  }
  process.env['npm_config_argv'] = JSON.stringify(argv)

  let config: Config & {
    forceSharedLockfile: boolean
    argv: { remain: string[], cooked: string[], original: string[] }
  }
  try {
    // When we just want to print the location of the global bin directory,
    // we don't need the write permission to it. Related issue: #2700
    const globalDirShouldAllowWrite = cmd !== 'root'
    config = await getConfig(cliOptions, {
      excludeReporter: false,
      globalDirShouldAllowWrite,
      rcOptionsTypes,
      workspaceDir,
      checkUnknownSetting: false,
    }) as typeof config
    config.forceSharedLockfile = typeof config.workspaceDir === 'string' && config.sharedWorkspaceLockfile === true
    config.argv = argv
  } catch (err) {
    // Reporting is not initialized at this point, so just printing the error
    const hint = err['hint'] ? err['hint'] : `For help, run: pnpm help${cmd ? ` ${cmd}` : ''}`
    printError(err.message, hint)
    process.exit(1)
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

  initReporter(reporterType, {
    cmd,
    config,
  })
  global['reporterInitialized'] = reporterType

  const selfUpdate = config.global && (cmd === 'add' || cmd === 'update') && cliParams.includes(packageManager.name)

  if (selfUpdate) {
    await pnpmCmds.server(config as any, ['stop']) // eslint-disable-line @typescript-eslint/no-explicit-any
    try {
      config.bin = path.dirname(which.sync('pnpm'))
    } catch (err) {
      // if pnpm not found, then ignore
    }
  }

  if (
    cmd === 'install' &&
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
    const allProjects = await findWorkspacePackages(wsDir, {
      engineStrict: config.engineStrict,
      patterns: cliOptions['workspace-packages'],
    })

    if (allProjects.length === 0) {
      if (!config['parseable']) {
        console.log(`No projects found in "${wsDir}"`)
      }
      process.exit(0)
    }

    config.filter = config.filter ?? []
    config.filterProd = config.filterProd ?? []

    const filters = [
      ...config.filter.map((filter) => ({ filter, followProdDepsOnly: false })),
      ...config.filterProd.map((filter) => ({ filter, followProdDepsOnly: true })),
    ]

    const filterResults = await filterPackages(allProjects, filters, {
      linkWorkspacePackages: !!config.linkWorkspacePackages,
      prefix: process.cwd(),
      workspaceDir: wsDir,
      testPattern: config.testPattern,
    })
    config.selectedProjectsGraph = filterResults.selectedProjectsGraph
    if (R.isEmpty(config.selectedProjectsGraph)) {
      if (!config['parseable']) {
        console.log(`No projects matched the filters in "${wsDir}"`)
      }
      process.exit(0)
    }
    if (filterResults.unmatchedFilters.length !== 0 && !config['parseable']) {
      console.log(`No projects matched the filters "${filterResults.unmatchedFilters.join(', ')}" in "${wsDir}"`)
    }
    config.allProjects = allProjects
    config.workspaceDir = wsDir
  }

  // NOTE: we defer the next stage, otherwise reporter might not catch all the logs
  const [output, exitCode] = await new Promise((resolve, reject) => {
    setTimeout(async () => {
      // Don't check for updates
      //   1. on CI environments
      //   2. when in the middle of an actual update
      //   3. when the CLI is running in offline mode
      if (!isCI && !selfUpdate && !config.offline && !config.preferOffline) {
        checkForUpdates(config).catch(() => { /* Ignore */ })
      }

      if (config.force === true) {
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

      try {
        if (config.useNodeVersion != null) {
          const nodePath = path.join(await node.getNodeDir(config.pnpmHomeDir, config.useNodeVersion), 'node_modules/.bin')
          config.extraBinPaths.push(nodePath)
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
        if (!result) {
          resolve([null, 0])
          return
        }
        if (typeof result === 'string') {
          resolve([result, 0])
          return
        }
        resolve([result['output'], result['exitCode']])
      } catch (err) {
        reject(err)
      }
    }, 0)
  })
  if (output) {
    write(output)
  }
  if (!cmd) {
    process.exit(1)
  }
  if (exitCode) {
    process.exit(exitCode)
  }
}

function printError (message: string, hint?: string) {
  console.error(`${chalk.bgRed.black('\u2009ERROR\u2009')} ${chalk.red(message)}`)
  if (hint) {
    console.log(hint)
  }
}
