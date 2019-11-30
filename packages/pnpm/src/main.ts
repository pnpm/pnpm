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
import { getConfig, packageManager } from '@pnpm/cli-utils'
import {
  Config,
  types as allTypes,
} from '@pnpm/config'
import { scopeLogger } from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import parseCliArgs from '@pnpm/parse-cli-args'
import isCI = require('is-ci')
import R = require('ramda')
import checkForUpdates from './checkForUpdates'
import pnpmCmds, { getCommandFullName, getCliOptionsTypes, getRCOptionsTypes } from './cmd'
import './logging/fileLogger'
import initReporter, { ReporterType } from './reporter'

const GLOBAL_OPTIONS = R.pick(['color', 'filter', 'help', 'dir', 'prefix'], allTypes)

const RENAMED_OPTIONS = {
  'lockfile-directory': 'lockfile-dir',
  'prefix': 'dir',
  'shrinkwrap-directory': 'lockfile-dir',
  'store': 'store-dir',
}

export default async function run (inputArgv: string[]) {
  // tslint:disable
  const shortHands = {
    's': ['--reporter', 'silent'],
    'd': ['--loglevel', 'info'],
    'dd': ['--loglevel', 'verbose'],
    'ddd': ['--loglevel', 'silly'],
    'L': ['--latest'],
    'r': ['--recursive'],
    'silent': ['--reporter', 'silent'],
    'verbose': ['--loglevel', 'verbose'],
    'quiet': ['--loglevel', 'warn'],
    'q': ['--loglevel', 'warn'],
    'h': ['--usage'],
    'H': ['--usage'],
    '?': ['--usage'],
    'help': ['--usage'],
    'v': ['--version'],
    'f': ['--force'],
    'local': ['--no-global'],
    'l': ['--long'],
    'p': ['--parseable'],
    'porcelain': ['--parseable'],
    'prod': ['--production'],
    'g': ['--global'],
    'S': ['--save'],
    'D': ['--save-dev'],
    'P': ['--save-prod'],
    'E': ['--save-exact'],
    'O': ['--save-optional'],
    'C': ['--dir'],
    'shrinkwrap-only': ['--lockfile-only'],
    'shared-workspace-shrinkwrap': ['--shared-workspace-lockfile'],
    'frozen-shrinkwrap': ['--frozen-lockfile'],
    'prefer-frozen-shrinkwrap': ['--prefer-frozen-lockfile'],
    'W': ['--ignore-workspace-root-check'],
  }
  // tslint:enable
  const { argv, cliArgs, cliConf, cmd, dir, subCmd, unknownOptions, workspaceDir } = await parseCliArgs({
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    globalOptionsTypes: GLOBAL_OPTIONS,
    isKnownCommand: (commandName) => typeof pnpmCmds[commandName] !== 'undefined',
    renamedOptions: RENAMED_OPTIONS,
    shortHands,
  }, inputArgv)
  if (unknownOptions.length > 0) {
    let errorMsg = `${chalk.bgRed.black('\u2009ERROR\u2009')}`
    if (unknownOptions.length === 1) {
      errorMsg += ` ${chalk.red(`Unknown option '${unknownOptions[0]}'`)}`
    } else {
      errorMsg += ` ${chalk.red(`Unknown options ${unknownOptions.map(unknownOption => `'${unknownOption}'`).join(', ')}`)}`
    }
    console.error(errorMsg)
    console.log(`For help, run: pnpm help ${cmd}`)
    process.exit(1)
  }
  process.env['npm_config_argv'] = JSON.stringify(argv)

  let config: Config & {
    forceSharedLockfile: boolean,
    argv: { remain: string[], cooked: string[], original: string[] },
  }
  try {
    config = await getConfig(cliConf, {
      command: subCmd ? [cmd, subCmd] : [cmd],
      excludeReporter: false,
      rcOptionsTypes: getRCOptionsTypes(cmd),
      workspaceDir,
    }) as typeof config
    config.forceSharedLockfile = typeof config.workspaceDir === 'string' && config.sharedWorkspaceLockfile === true
    config.argv = argv
  } catch (err) {
    // Reporting is not initialized at this point, so just printing the error
    console.error(`${chalk.bgRed.black('\u2009ERROR\u2009')} ${chalk.red(err.message)}`)
    console.log(`For help, run: pnpm help ${cmd}`)
    process.exit(1)
    return
  }

  // chalk reads the FORCE_COLOR env variable
  if (config.color === 'always') {
    process.env['FORCE_COLOR'] = '1'
  } else if (config.color === 'never') {
    process.env['FORCE_COLOR'] = '0'
  }

  if (
    cmd === 'add' &&
    workspaceDir === dir &&
    !config.ignoreWorkspaceRootCheck
  ) {
    // Reporting is not initialized at this point, so just printing the error
    console.error(`${chalk.bgRed.black('\u2009ERROR\u2009')} ${
      chalk.red('Running this command will add the dependency to the workspace root, ' +
        'which might not be what you want - if you really meant it, ' +
        'make it explicit by running this command again with the -W flag (or --ignore-workspace-root-check).')}`)
    console.log(`For help, run: pnpm help ${cmd}`)
    process.exit(1)
    return
  }

  const selfUpdate = config.global && (cmd === 'add' || cmd === 'update') && argv.remain.includes(packageManager.name)

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
    subCmd,
  })
  delete config.reporter // This is a silly workaround because supi expects a function as config.reporter

  if (selfUpdate) {
    await pnpmCmds.server(['stop'], config as any) // tslint:disable-line:no-any
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

      if (cmd !== 'recursive') {
        scopeLogger.debug(workspaceDir
          ? { selected: 1, workspacePrefix: workspaceDir }
          : { selected: 1 })
      }

      try {
        const result = pnpmCmds[cmd](
          cliArgs,
          // TypeScript doesn't currently infer that the type of config
          // is `Omit<typeof config, 'reporter'>` after the `delete config.reporter` statement
          config as Omit<typeof config, 'reporter'>,
          argv.remain[0]
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
}
