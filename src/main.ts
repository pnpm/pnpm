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
import logger from '@pnpm/logger'
import camelcase = require('camelcase')
import {stripIndent} from 'common-tags'
import isCI = require('is-ci')
import nopt = require('nopt')
import loadNpmConf = require('npm-conf')
import npmTypes = require('npm-conf/lib/types')
import path = require('path')
import R = require('ramda')
import checkForUpdates from './checkForUpdates'
import * as pnpmCmds from './cmd'
import runNpm from './cmd/runNpm'
import getCommandFullName from './getCommandFullName'
import './logging/fileLogger'
import pkg from './pnpmPkgJson'
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
  'dislink',
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
  const knownOpts = Object.assign({
    'background': Boolean,
    'child-concurrency': Number,
    'fetching-concurrency': Number,
    'global-path': path,
    'ignore-pnpmfile': Boolean,
    'ignore-stop-requests': Boolean,
    'ignore-upload-requests': Boolean,
    'independent-leaves': Boolean,
    'lock': Boolean,
    'lock-stale-duration': Number,
    'network-concurrency': Number,
    'offline': Boolean,
    'package-import-method': ['auto', 'hardlink', 'reflink', 'copy'],
    'pending': Boolean,
    'port': Number,
    'prefer-offline': Boolean,
    'protocol': ['auto', 'tcp', 'ipc'],
    'reporter': String,
    'shrinkwrap-only': Boolean,
    'side-effects-cache': Boolean,
    'side-effects-cache-readonly': Boolean,
    'store': path,
    'store-path': path, // DEPRECATE! store should be used
    'use-store-server': Boolean,
    'verify-store-integrity': Boolean,
  }, npmTypes.types)
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
    'g': ['--global'],
    'S': ['--save'],
    'D': ['--save-dev'],
    'E': ['--save-exact'],
    'O': ['--save-optional'],
    'y': ['--yes'],
    'n': ['--no-yes'],
    'B': ['--save-bundle'],
    'C': ['--prefix'],
  }
  // tslint:enable
  const cliConf = nopt(knownOpts, shortHands, argv, 0)

  if (!isCI) {
    checkForUpdates()
  }

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
  if (!cliConf['user-agent']) {
    cliConf['user-agent'] = `${pkg.name}/${pkg.version} npm/? node/${process.version} ${process.platform} ${process.arch}`
  }

  const npmConfig = loadNpmConf()

  const opts = R.fromPairs(<any>Object.keys(knownOpts).map(configKey => [ // tslint:disable-line
    camelcase(configKey),
    typeof cliConf[configKey] !== 'undefined' ? cliConf[configKey] : npmConfig.get(configKey),
  ]))
  opts.rawNpmConfig = Object.assign.apply(Object, npmConfig.list.reverse().concat([cliConf]))
  opts.globalBin = process.platform === 'win32'
    ? npmConfig.globalPrefix
    : path.resolve(npmConfig.globalPrefix, 'bin')
  opts.bin = opts.global
    ? opts.globalBin
    : path.join(npmConfig.localPrefix, 'node_modules', '.bin')
  opts.globalPrefix = path.join(npmConfig.globalPrefix, 'pnpm-global')
  opts.prefix = opts.global ? opts.globalPrefix : npmConfig.prefix
  opts.packageManager = pkg

  if (opts.only === 'prod' || opts.only === 'production' || !opts.only && opts.production) {
    opts.production = true
    opts.development = false
  } else if (opts.only === 'dev' || opts.only === 'development') {
    opts.production = false
    opts.development = true
    opts.optional = false
  } else {
    opts.production = true
    opts.development = true
  }

  if (!opts.packageLock && opts.shrinkwrap) {
    opts.shrinkwrap = false
  }

  const reporterType: ReporterType = (() => {
    if (npmConfig.get('loglevel') === 'silent') return 'silent'
    if (opts.reporter) return opts.reporter as ReporterType
    if (isCI || !process.stdout.isTTY) return 'append-only'
    return 'default'
  })()
  initReporter(reporterType, cmd) // tslint:disable-line
  delete opts.reporter // This is a silly workaround because supi expects a function as opts.reporter

  // NOTE: we defer the next stage, otherwise reporter might not catch all the logs
  await new Promise((resolve, reject) => {
    setTimeout(() => {
      if (opts.storePath && !opts.store) {
        logger.warn('the `store-path` config is deprecated. Use `store` instead.')
        opts.store = opts.storePath
      }

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
