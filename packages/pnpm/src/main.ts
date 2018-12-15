// Map SIGINT & SIGTERM to process exit
// so that lockfiles are removed automatically
process
  .once('SIGINT', () => process.exit(1))
  .once('SIGTERM', () => process.exit(1))

// Patch the global fs module here at the app level
import fs = require('fs')
import gfs = require('graceful-fs')

gfs.gracefulify(fs)

import loudRejection = require('loud-rejection')
loudRejection()
import {
  PnpmConfigs,
  types,
} from '@pnpm/config'
import { cliLogger } from '@pnpm/core-loggers'
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
  | 'import'
  | 'install-test'
  | 'install'
  | 'link'
  | 'list'
  | 'outdated'
  | 'prune'
  | 'rebuild'
  | 'recursive'
  | 'root'
  | 'run'
  | 'server'
  | 'store'
  | 'test'
  | 'uninstall'
  | 'unlink'
  | 'update'

const COMMANDS_WITH_NO_DASHDASH_FILTER = new Set(['run', 'exec', 'test'])

const supportedCmds = new Set<CANONICAL_COMMAND_NAMES>([
  'install',
  'uninstall',
  'update',
  'link',
  'prune',
  'install-test',
  'server',
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
    'noreg': ['--no-registry'],
    'N': ['--no-registry'],
    'reg': ['--registry'],
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

  let opts!: PnpmConfigs
  try {
    opts = await getConfigs(cliConf, { excludeReporter: false })
    opts.include = {
      dependencies: opts.production !== false,
      devDependencies: opts.development !== false,
      optionalDependencies: opts.optional !== false,
    }
    opts.forceSharedShrinkwrap = typeof opts.workspacePrefix === 'string' && opts.sharedWorkspaceShrinkwrap === true
  } catch (err) {
    // Reporting is not initialized at this point, so just printing the error
    console.error(err.message)
    process.exit(1)
    return
  }

  const selfUpdate = opts.global && (cmd === 'install' || cmd === 'update') && cliConf.argv.remain.indexOf(packageManager.name) !== -1

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

  let subCmd = cliConf.argv.remain[1] && getCommandFullName(cliConf.argv.remain[1])

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

      const dashDashFilterUsed = (
        (
          cmd === 'recursive' && !COMMANDS_WITH_NO_DASHDASH_FILTER.has(subCmd)
          || cmd !== 'recursive' && !COMMANDS_WITH_NO_DASHDASH_FILTER.has(cmd)
        )
        && cliConf.argv.cooked.indexOf('--') !== -1
      )

      if (dashDashFilterUsed) {
        opts.filter = opts.filter || []
        const dashDashIndex = cliConf.argv.cooked.indexOf('--')
        Array.prototype.push.apply(opts.filter, cliConf.argv.cooked.slice(dashDashIndex + 1))
        const afterDashDash = cliConf.argv.cooked.length - dashDashIndex - 1
        cliConf.argv.remain = cliConf.argv.remain.slice(0, cliConf.argv.remain.length - afterDashDash)
      }

      // `pnpm install ""` is going to be just `pnpm install`
      const cliArgs = cliConf.argv.remain.slice(1).filter(Boolean)

      if (cmd !== 'recursive' && (dashDashFilterUsed || argv.indexOf('--filter') !== -1)) {
        subCmd = cmd
        cmd = 'recursive'
        cliArgs.unshift(subCmd)
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
            .then(resolve)
            .catch(reject)
        } else {
          resolve()
        }
      } catch (err) {
        reject(err)
      }
    }, 0)
  })

  cliLogger.debug('command_done')
}
