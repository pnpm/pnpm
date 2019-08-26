// Map SIGINT & SIGTERM to process exit
// so that lockfiles are removed automatically
process
  .once('SIGINT', () => process.exit(0))
  .once('SIGTERM', () => process.exit(0))

// Patch the global fs module here at the app level
import chalk from 'chalk'
import fs = require('fs')
import gfs = require('graceful-fs')

gfs.gracefulify(fs)

import loudRejection from 'loud-rejection'
loudRejection()
import {
  PnpmConfigs,
  types,
} from '@pnpm/config'
import logger from '@pnpm/logger'
import isCI = require('is-ci')
import nopt = require('nopt')
import checkForUpdates from './checkForUpdates'
import pnpmCmds from './cmd'
import getCommandFullName from './getCommandFullName'
import getConfigs from './getConfigs'
import { scopeLogger } from './loggers'
import './logging/fileLogger'
import packageManager from './pnpmPkgJson'
import initReporter, { ReporterType } from './reporter'

pnpmCmds['install-test'] = pnpmCmds.installTest

type CANONICAL_COMMAND_NAMES = 'help'
  | 'add'
  | 'import'
  | 'install-test'
  | 'install'
  | 'link'
  | 'list'
  | 'outdated'
  | 'pack'
  | 'prune'
  | 'publish'
  | 'rebuild'
  | 'recursive'
  | 'restart'
  | 'root'
  | 'run'
  | 'server'
  | 'start'
  | 'stop'
  | 'store'
  | 'test'
  | 'uninstall'
  | 'unlink'
  | 'update'

const COMMANDS_WITH_NO_DASHDASH_FILTER = new Set(['run', 'exec', 'restart', 'start', 'stop', 'test'])

const supportedCmds = new Set<CANONICAL_COMMAND_NAMES>([
  'add',
  'install',
  'uninstall',
  'update',
  'link',
  'pack',
  'prune',
  'publish',
  'install-test',
  'restart',
  'server',
  'start',
  'stop',
  'store',
  'list',
  'unlink',
  'help',
  'root',
  'outdated',
  'rebuild',
  'recursive',
  'import',
  'test',
  'run',
  // These might have to be implemented:
  // 'cache',
  // 'completion',
  // 'explore',
  // 'dedupe',
  // 'doctor',
  // 'shrinkwrap',
  // 'help-search',
])

export default async function run (argv: string[]) {
  // tslint:disable
  const shortHands = {
    's': ['--loglevel', 'silent'],
    'd': ['--loglevel', 'info'],
    'dd': ['--loglevel', 'verbose'],
    'ddd': ['--loglevel', 'silly'],
    'L': ['--latest'],
    'noreg': ['--no-registry'],
    'N': ['--no-registry'],
    'r': ['--recursive'],
    'no-reg': ['--no-registry'],
    'silent': ['--loglevel', 'silent'],
    'verbose': ['--loglevel', 'verbose'],
    'quiet': ['--loglevel', 'warn'],
    'q': ['--loglevel', 'warn'],
    'h': ['--usage'],
    'H': ['--usage'],
    '?': ['--usage'],
    'help': ['--usage'],
    'v': ['--version'],
    'f': ['--force'],
    'desc': ['--description'],
    'no-desc': ['--no-description'],
    'local': ['--no-global'],
    'l': ['--long'],
    'm': ['--message'],
    'p': ['--parseable'],
    'porcelain': ['--parseable'],
    'prod': ['--production'],
    'g': ['--global'],
    'S': ['--save'],
    'D': ['--save-dev'],
    'P': ['--save-prod'],
    'E': ['--save-exact'],
    'O': ['--save-optional'],
    'y': ['--yes'],
    'n': ['--no-yes'],
    'B': ['--save-bundle'],
    'C': ['--prefix'],
    'lockfile-directory': ['--shrinkwrap-directory'],
    'lockfile-only': ['--shrinkwrap-only'],
    'shared-workspace-lockfile': ['--shared-workspace-shrinkwrap'],
    'frozen-lockfile': ['--frozen-shrinkwrap'],
    'prefer-frozen-lockfile': ['--prefer-frozen-shrinkwrap'],
    'W': ['--ignore-workspace-root-check'],
  }
  // tslint:enable
  const cliConf = nopt(types, shortHands, argv, 0)

  let cmd = getCommandFullName(cliConf.argv.remain[0]) as CANONICAL_COMMAND_NAMES
    || 'help'
  if (!supportedCmds.has(cmd)) {
    cmd = 'help'
  }

  if (cliConf['dry-run']) {
    console.error(`Error: 'dry-run' is not supported yet, sorry!`)
    process.exit(1)
  }

  cliConf.save = cliConf.save || !cliConf['save-dev'] && !cliConf['save-optional']
  let subCmd = cliConf.argv.remain[1] && getCommandFullName(cliConf.argv.remain[1])

  const dashDashFilterUsed = (
    (
      cmd === 'recursive' && !COMMANDS_WITH_NO_DASHDASH_FILTER.has(subCmd)
      || cmd !== 'recursive' && !COMMANDS_WITH_NO_DASHDASH_FILTER.has(cmd)
    )
    && cliConf.argv.cooked.includes('--')
  )

  const filterArgs = [] as string[]

  if (dashDashFilterUsed) {
    const dashDashIndex = cliConf.argv.cooked.indexOf('--')
    Array.prototype.push.apply(filterArgs, cliConf.argv.cooked.slice(dashDashIndex + 1))
    const afterDashDash = cliConf.argv.cooked.length - dashDashIndex - 1
    cliConf.argv.remain = cliConf.argv.remain.slice(0, cliConf.argv.remain.length - afterDashDash)
  }

  // `pnpm install ""` is going to be just `pnpm install`
  const cliArgs = cliConf.argv.remain.slice(1).filter(Boolean)

  if (cmd !== 'recursive' && (dashDashFilterUsed || argv.includes('--filter') || cliConf['recursive'] === true)) {
    subCmd = cmd
    cmd = 'recursive'
    cliArgs.unshift(subCmd)
  }

  let opts!: PnpmConfigs
  try {
    opts = await getConfigs(cliConf, {
      command: subCmd ? [cmd, subCmd] : [cmd],
      excludeReporter: false,
    })
    opts.forceSharedLockfile = typeof opts.workspacePrefix === 'string' && opts.sharedWorkspaceLockfile === true
    opts.argv = cliConf.argv
    if (opts.filter) {
      Array.prototype.push.apply(opts.filter, filterArgs)
    } else {
      opts.filter = filterArgs
    }
  } catch (err) {
    // Reporting is not initialized at this point, so just printing the error
    console.error(`${chalk.bgRed.black('\u2009ERROR\u2009')} ${chalk.red(err.message)}`)
    console.log(`For help, run: pnpm help ${cmd}`)
    process.exit(1)
    return
  }

  if (
    opts.useBetaCli &&
    (cmd === 'add' || cmd === 'install') &&
    typeof opts.workspacePrefix === 'string'
  ) {
    if (cliArgs.length === 0) {
      subCmd = cmd
      cmd = 'recursive'
      cliArgs.unshift(subCmd)
    } else if (
      opts.workspacePrefix === opts.prefix &&
      !opts.ignoreWorkspaceRootCheck
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
  }

  const selfUpdate = opts.global && (cmd === 'install' || cmd === 'update') && cliConf.argv.remain.includes(packageManager.name)

  // Don't check for updates
  //   1. on CI environments
  //   2. when in the middle of an actual update
  if (!isCI && !selfUpdate) {
    checkForUpdates()
  }

  const reporterType: ReporterType = (() => {
    if (opts.loglevel === 'silent') return 'silent'
    if (opts.reporter) return opts.reporter as ReporterType
    if (isCI || !process.stdout.isTTY) return 'append-only'
    return 'default'
  })()

  initReporter(reporterType, {
    cmd,
    pnpmConfigs: opts,
    subCmd,
  })
  delete opts.reporter // This is a silly workaround because supi expects a function as opts.reporter

  if (selfUpdate) {
    await pnpmCmds.server(['stop'], opts as any) // tslint:disable-line:no-any
  }

  // NOTE: we defer the next stage, otherwise reporter might not catch all the logs
  await new Promise((resolve, reject) => {
    setTimeout(() => {
      if (cliConf['shamefully-flatten'] === true) {
        logger.info({
          message: 'Installing a flat node_modules. Use flat node_modules only if you rely on buggy dependencies that you cannot fix.',
          prefix: opts.prefix,
        })
      }
      if (opts.force === true) {
        logger.warn({
          message: 'using --force I sure hope you know what you are doing',
          prefix: opts.prefix,
        })
      }

      if (cmd !== 'recursive') {
        scopeLogger.debug({
          selected: 1,
          workspacePrefix: opts.workspacePrefix,
        })
      }

      try {
        const result = pnpmCmds[cmd](cliArgs, opts, cliConf.argv.remain[0])
        if (result instanceof Promise) {
          result
            .then((output) => {
              if (typeof output === 'string') {
                console.log(output)
              }
              resolve()
            })
            .catch(reject)
        } else {
          if (typeof result === 'string') {
            console.log(result)
          }
          resolve()
        }
      } catch (err) {
        reject(err)
      }
    }, 0)
  })
}
