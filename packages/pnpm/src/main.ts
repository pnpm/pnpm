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
  | 'why'

type CLI_OPTIONS = 'access'
  | 'background'
  | 'bail'
  | 'child-concurrency'
  | 'depth'
  | 'dev'
  | 'engine-strict'
  | 'filter'
  | 'force'
  | 'frozen-lockfile'
  | 'global-pnpmfile'
  | 'global'
  | 'help'
  | 'hoist-pattern'
  | 'hoist'
  | 'ignore-pnpmfile'
  | 'ignore-scripts'
  | 'ignore-stop-requests'
  | 'ignore-upload-requests'
  | 'ignore-workspace-root-check'
  | 'independent-leaves'
  | 'json'
  | 'latest'
  | 'link-workspace-packages'
  | 'lockfile-directory'
  | 'lockfile-only'
  | 'lockfile'
  | 'long'
  | 'network-concurrency'
  | 'offline'
  | 'only'
  | 'optional'
  | 'package-import-method'
  | 'parseable'
  | 'pnpmfile'
  | 'port'
  | 'prefer-frozen-lockfile'
  | 'prefer-offline'
  | 'prefix'
  | 'production'
  | 'protocol'
  | 'recursive'
  | 'registry'
  | 'reporter'
  | 'resolution-strategy'
  | 'save-dev'
  | 'save-exact'
  | 'save-optional'
  | 'save-peer'
  | 'save-prod'
  | 'shamefully-flatten'
  | 'shamefully-hoist'
  | 'shared-workspace-lockfile'
  | 'side-effects-cache-readonly'
  | 'side-effects-cache'
  | 'silent'
  | 'sort'
  | 'store'
  | 'strict-peer-dependencies'
  | 'table'
  | 'use-running-store-server'
  | 'use-store-server'
  | 'verify-store-integrity'
  | 'workspace-concurrency'
  | 'workspace-prefix'

const GLOBAL_OPTIONS = new Set<CLI_OPTIONS>(['filter', 'help', 'prefix'])

const INSTALL_CLI_OPTIONS = new Set<CLI_OPTIONS>([
  'child-concurrency',
  'dev',
  'engine-strict',
  'frozen-lockfile',
  'force',
  'global-pnpmfile',
  'global',
  'hoist',
  'hoist-pattern',
  'ignore-pnpmfile',
  'ignore-scripts',
  'ignore-workspace-root-check',
  'independent-leaves',
  'link-workspace-packages',
  'lockfile-directory',
  'lockfile-only',
  'lockfile',
  'package-import-method',
  'pnpmfile',
  'prefer-frozen-lockfile',
  'prefer-offline',
  'production',
  'recursive',
  'registry',
  'reporter',
  'save-dev',
  'save-exact',
  'save-optional',
  'save-peer',
  'save-prod',
  'shamefully-hoist',
  'shared-workspace-lockfile',
  'side-effects-cache-readonly',
  'side-effects-cache',
  'store',
  'strict-peer-dependencies',
  'offline',
  'only',
  'optional',
  'use-running-store-server',
  'use-store-server',
  'verify-store-integrity',
  'workspace-prefix',
])

const SUPPORTED_CLI_OPTIONS: Record<CANONICAL_COMMAND_NAMES, Set<CLI_OPTIONS>> = {
  'add': INSTALL_CLI_OPTIONS,
  'help': new Set([]),
  'import': new Set([]),
  'install': INSTALL_CLI_OPTIONS,
  'install-test': INSTALL_CLI_OPTIONS,
  'link': new Set([
    'only',
    'package-import-method',
    'production',
    'registry',
    'reporter',
    'save-dev',
    'save-exact',
    'save-optional',
  ]),
  'list': new Set([
    'depth',
    'dev',
    'global',
    'json',
    'long',
    'only',
    'optional',
    'parseable',
    'production',
    'recursive',
  ]),
  'outdated': new Set([
    'depth',
    'global',
    'long',
    'recursive',
    'table',
  ]),
  'pack': new Set([]),
  'prune': new Set([]),
  'publish': new Set([]),
  'rebuild': new Set([
    'recursive',
  ]),
  'recursive': new Set([
    'bail',
    'link-workspace-packages',
    'shared-workspace-lockfile',
    'sort',
    'workspace-concurrency',
  ]),
  'restart': new Set([]),
  'root': new Set([
    'global',
  ]),
  'run': new Set([
    'recursive',
  ]),
  'server': new Set([
    'background',
    'ignore-stop-requests',
    'ignore-upload-requests',
    'port',
    'protocol',
    'store',
  ]),
  'start': new Set([]),
  'stop': new Set([]),
  'store': new Set([
    'registry',
  ]),
  'test': new Set([
    'recursive',
  ]),
  'uninstall': new Set([
    'force',
    'global-pnpmfile',
    'global',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'package-import-method',
    'pnpmfile',
    'recursive',
    'reporter',
    'shared-workspace-lockfile',
    'store',
  ]),
  'unlink': new Set([
    'recursive',
  ]),
  'update': new Set([
    'depth',
    'dev',
    'engine-strict',
    'force',
    'global-pnpmfile',
    'ignore-pnpmfile',
    'ignore-scripts',
    'latest',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'offline',
    'only',
    'optional',
    'package-import-method',
    'pnpmfile',
    'prefer-offline',
    'production',
    'recursive',
    'registry',
    'reporter',
    'save-exact',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store',
    'use-running-store-server',
  ]),
  'why': new Set([
    'recursive',
  ]),
}

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
  'why',
  // These might have to be implemented:
  // 'cache',
  // 'completion',
  // 'explore',
  // 'dedupe',
  // 'doctor',
  // 'shrinkwrap',
  // 'help-search',
])

export default async function run (inputArgv: string[]) {
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
  const { argv, ...cliConf } = nopt(types, shortHands, inputArgv, 0)
  process.env['npm_config_argv'] = JSON.stringify(argv)

  let cmd = getCommandFullName(argv.remain[0]) as CANONICAL_COMMAND_NAMES
    || 'help'
  if (!supportedCmds.has(cmd)) {
    cmd = 'help'
  }

  if (cliConf['dry-run']) {
    console.error(`Error: 'dry-run' is not supported yet, sorry!`)
    process.exit(1)
  }

  let subCmd: string | null = argv.remain[1] && getCommandFullName(argv.remain[1])

  const dashDashFilterUsed = (
    (
      cmd === 'recursive' && !COMMANDS_WITH_NO_DASHDASH_FILTER.has(subCmd)
      || cmd !== 'recursive' && !COMMANDS_WITH_NO_DASHDASH_FILTER.has(cmd)
    )
    && argv.cooked.includes('--')
  )

  const filterArgs = [] as string[]

  if (dashDashFilterUsed) {
    const dashDashIndex = argv.cooked.indexOf('--')
    Array.prototype.push.apply(filterArgs, argv.cooked.slice(dashDashIndex + 1))
    const afterDashDash = argv.cooked.length - dashDashIndex - 1
    argv.remain = argv.remain.slice(0, argv.remain.length - afterDashDash)
  }

  // `pnpm install ""` is going to be just `pnpm install`
  const cliArgs = argv.remain.slice(1).filter(Boolean)

  if (cmd !== 'recursive' && (dashDashFilterUsed || inputArgv.includes('--filter') || cliConf['recursive'] === true)) {
    subCmd = cmd
    cmd = 'recursive'
    cliArgs.unshift(subCmd)
  } else if (subCmd && !supportedCmds.has(subCmd as CANONICAL_COMMAND_NAMES)) {
    subCmd = null
  }

  const allowedOptions = !subCmd
    ? SUPPORTED_CLI_OPTIONS[cmd]
    : new Set([...Array.from(SUPPORTED_CLI_OPTIONS[cmd]), ...Array.from(SUPPORTED_CLI_OPTIONS[subCmd])])
  for (const cliOption of Object.keys(cliConf)) {
    if (!GLOBAL_OPTIONS.has(cliOption as CLI_OPTIONS) && !allowedOptions.has(cliOption)) {
      console.error(`${chalk.bgRed.black('\u2009ERROR\u2009')} ${chalk.red(`Unknown option '${cliOption}'`)}`)
      console.log(`For help, run: pnpm help ${cmd}`)
      process.exit(1)
      return
    }
  }

  let opts!: PnpmConfigs
  try {
    opts = await getConfigs(cliConf, {
      command: subCmd ? [cmd, subCmd] : [cmd],
      excludeReporter: false,
    })
    opts.forceSharedLockfile = typeof opts.workspacePrefix === 'string' && opts.sharedWorkspaceLockfile === true
    opts.argv = argv
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

  const selfUpdate = opts.global && (cmd === 'install' || cmd === 'update') && argv.remain.includes(packageManager.name)

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
        const result = pnpmCmds[cmd](cliArgs, opts, argv.remain[0])
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
