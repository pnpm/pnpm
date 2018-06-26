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
import getConfigs, {types} from '@pnpm/config'
import logger from '@pnpm/logger'
import camelcase = require('camelcase')
import {stripIndent} from 'common-tags'
import isCI = require('is-ci')
import nopt = require('nopt')
import path = require('path')
import checkForUpdates from './checkForUpdates'
import * as pnpmCmds from './cmd'
import runNpm from './cmd/runNpm'
import getCommandFullName from './getCommandFullName'
import './logging/fileLogger'
import packageManager from './pnpmPkgJson'
import initReporter, { ReporterType } from './reporter'

pnpmCmds['install-test'] = pnpmCmds.installTest

const supportedCmds = new Set([
  'add',
  'install',
  'uninstall',
  'update',
  'link',
  'prune',
  'install-test',
  'run',
  'server',
  'store',
  'list',
  'unlink',
  'help',
  'root',
  'outdated',
  'rebuild',
  'recursive',
  // These might have to be implemented:
  // 'cache',
  // 'completion',
  // 'explore',
  // 'dedupe',
  // 'doctor',
  // 'shrinkwrap',
  // 'help-search',
])

const passedThroughCmds = new Set([
  'access',
  'adduser',
  'bin',
  'bugs',
  'c',
  'config',
  'deprecate',
  'dist-tag',
  'docs',
  'edit',
  'get',
  'info',
  'init',
  'login',
  'logout',
  'owner',
  'pack',
  'ping',
  'prefix',
  'profile',
  'publish',
  'repo',
  'restart',
  's',
  'se',
  'search',
  'set',
  'star',
  'stars',
  'start',
  'stop',
  'team',
  't',
  'tst',
  'test',
  'token',
  'unpublish',
  'unstar',
  'v',
  'version',
  'view',
  'whoami',
  'xmas',
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

  let cmd = getCommandFullName(cliConf.argv.remain[0]) || 'help'
  if (!supportedCmds.has(cmd)) {
    if (passedThroughCmds.has(cmd)) {
      runNpm(argv)
      return Promise.resolve()
    }
    cmd = 'help'
  }

  if (cliConf['dry-run']) {
    console.error(`Error: 'dry-run' is not supported yet, sorry!`)
    process.exit(1)
  }

  cliConf.save = cliConf.save || !cliConf['save-dev'] && !cliConf['save-optional']

  const opts = await getConfigs({
    cliArgs: cliConf,
    packageManager,
  })

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
  initReporter(reporterType, cmd, cliConf.argv.remain[1] && getCommandFullName(cliConf.argv.remain[1])) // tslint:disable-line
  delete opts.reporter // This is a silly workaround because supi expects a function as opts.reporter

  if (selfUpdate) {
    await pnpmCmds.server(['stop'], opts)
  }

  // NOTE: we defer the next stage, otherwise reporter might not catch all the logs
  await new Promise((resolve, reject) => {
    setTimeout(() => {
      // `pnpm install ""` is going to be just `pnpm install`
      const cliArgs = cliConf.argv.remain.slice(1).filter(Boolean)
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
  logger('cli').debug('command_done')
}
