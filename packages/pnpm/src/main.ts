// Map SIGINT & SIGTERM to process exit
// so that lockfiles are removed automatically
process
  .once('SIGINT', () => process.exit(0))
  .once('SIGTERM', () => process.exit(0))

// Patch the global fs module here at the app level
import chalk = require('chalk')
import fs = require('fs')
import gfs = require('graceful-fs')

gfs.gracefulify(fs)

import loudRejection from 'loud-rejection'
loudRejection()
import packageManager from '@pnpm/cli-meta'
import { getConfig } from '@pnpm/cli-utils'
import {
  Config,
} from '@pnpm/config'
import { scopeLogger } from '@pnpm/core-loggers'
import { filterPackages } from '@pnpm/filter-workspace-packages'
import findWorkspacePackages from '@pnpm/find-workspace-packages'
import logger from '@pnpm/logger'
import isCI = require('is-ci')
import path = require('path')
import R = require('ramda')
import which = require('which')
import checkForUpdates from './checkForUpdates'
import pnpmCmds, { getRCOptionsTypes } from './cmd'
import { formatUnknownOptionsError } from './formatError'
import './logging/fileLogger'
import parseCliArgs from './parseCliArgs'
import initReporter, { ReporterType } from './reporter'

const DEPRECATED_OPTIONS = new Set([
  'independent-leaves',
  'lock',
  'resolution-strategy',
])

export default async function run (inputArgv: string[]) {
  const {
    argv,
    params: cliParams,
    options: cliOptions,
    cmd,
    unknownOptions,
    workspaceDir,
  } = await parseCliArgs(inputArgv)
  if (cmd !== null && !pnpmCmds[cmd]) {
    console.error(`${chalk.bgRed.black('\u2009ERROR\u2009')} ${chalk.red(`Unknown command '${cmd}'`)}`)
    console.log(`For help, run: pnpm help`)
    process.exit(1)
  }

  if (unknownOptions.size > 0) {
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
      console.error(formatUnknownOptionsError(unknownOptions))
      console.log(`For help, run: pnpm help${cmd ? ` ${cmd}` : ''}`)
      process.exit(1)
    }
  }
  process.env['npm_config_argv'] = JSON.stringify(argv)

  let config: Config & {
    forceSharedLockfile: boolean,
    argv: { remain: string[], cooked: string[], original: string[] },
  }
  try {
    config = await getConfig(cliOptions, {
      excludeReporter: false,
      rcOptionsTypes: getRCOptionsTypes(cmd),
      workspaceDir,
    }) as typeof config
    config.forceSharedLockfile = typeof config.workspaceDir === 'string' && config.sharedWorkspaceLockfile === true
    config.argv = argv
  } catch (err) {
    // Reporting is not initialized at this point, so just printing the error
    console.error(`${chalk.bgRed.black('\u2009ERROR\u2009')} ${chalk.red(err.message)}`)
    if (err['hint']) {
      console.log(err['hint'])
    } else {
      console.log(`For help, run: pnpm help${cmd ? ` ${cmd}` : ''}`)
    }
    process.exit(1)
    return
  }

  // chalk reads the FORCE_COLOR env variable
  if (config.color === 'always') {
    process.env['FORCE_COLOR'] = '1'
  } else if (config.color === 'never') {
    process.env['FORCE_COLOR'] = '0'
  }

  const selfUpdate = config.global && (cmd === 'add' || cmd === 'update') && cliParams.includes(packageManager.name)

  // Don't check for updates
  //   1. on CI environments
  //   2. when in the middle of an actual update
  if (!isCI && !selfUpdate) {
    checkForUpdates()
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

  if (selfUpdate) {
    await pnpmCmds.server(config as any, ['stop']) // tslint:disable-line:no-any
    try {
      config.bin = path.dirname(which.sync('pnpm'))
    } catch (err) {
      // if pnpm not found, then ignore
    }
  }

  if (cliOptions['recursive']) {
    const wsDir = workspaceDir ?? process.cwd()
    const allProjects = await findWorkspacePackages(wsDir, {
      engineStrict: config.engineStrict,
      patterns: cliOptions['workspace-packages'],
    })

    if (!allProjects.length) {
      if (!config['parseable']) {
        console.log(`No projects found in "${wsDir}"`)
      }
      process.exit(0)
      return
    }
    const filterResults = await filterPackages(allProjects, config.filter ?? [], {
      linkWorkspacePackages: !!config.linkWorkspacePackages,
      prefix: process.cwd(),
      workspaceDir: wsDir,
    })
    config.selectedProjectsGraph = filterResults.selectedProjectsGraph
    if (R.isEmpty(config.selectedProjectsGraph)) {
      if (!config['parseable']) {
        console.log(`No projects matched the filters in "${wsDir}"`)
      }
      process.exit(0)
      return
    }
    if (filterResults.unmatchedFilters.length !== 0 && !config['parseable']) {
      console.log(`No projects matched the filters "${filterResults.unmatchedFilters.join(', ')}" in "${wsDir}"`)
    }
    config.allProjects = allProjects
    config.workspaceDir = wsDir
  }

  // NOTE: we defer the next stage, otherwise reporter might not catch all the logs
  await new Promise((resolve, reject) => {
    setTimeout(() => {
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
        const result = pnpmCmds[cmd || 'help'](
          // TypeScript doesn't currently infer that the type of config
          // is `Omit<typeof config, 'reporter'>` after the `delete config.reporter` statement
          config as Omit<typeof config, 'reporter'>,
          cliParams
        )
        if (result instanceof Promise) {
          result
            .then((output) => {
              if (typeof output === 'string') {
                process.stdout.write(output)
              }
              resolve()
            })
            .catch(reject)
        } else {
          if (typeof result === 'string') {
            process.stdout.write(result)
          }
          resolve()
        }
      } catch (err) {
        reject(err)
      }
    }, 0)
  })
  if (!cmd) {
    process.exit(1)
  }
}
