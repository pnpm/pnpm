#!/usr/bin/env node

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
import camelcase = require('camelcase')
import {stripIndent} from 'common-tags'
import isCI = require('is-ci')
import nopt = require('nopt')
import npm = require('not-bundled-npm')
import npmDefaults = require('not-bundled-npm/lib/config/defaults')
import path = require('path')
import R = require('ramda')
import checkForUpdates from '../checkForUpdates'
import * as pnpmCmds from '../cmd'
import runNpm from '../cmd/runNpm'
import getCommandFullName from '../getCommandFullName'
import '../logging/fileLogger'
import pkg from '../pnpmPkgJson'
import initReporter from '../reporter'

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
])

async function run (argv: string[]) {
  if (argv.indexOf('--help') !== -1 || argv.indexOf('-h') !== -1 || argv.indexOf('--h') !== -1) {
    argv = ['help'].concat(argv)
  }
  const pnpmTypes = {
    'child-concurrency': Number,
    'fetching-concurrency': Number,
    'global-path': path,
    'independent-leaves': Boolean,
    'lock': Boolean,
    'lock-stale-duration': Number,
    'network-concurrency': Number,
    'offline': Boolean,
    'reporter': String,
    'store': path,
    'store-path': path, // DEPRECATE! store should be used
    'verify-store-integrity': Boolean,
  }
  const types = R.merge(npmDefaults.types, pnpmTypes)
  const cliConf = nopt(
    types,
    npmDefaults.shorthands,
    argv,
    0, // argv should be already sliced by now
  )

  if (cliConf.version) {
    console.log(pkg.version)
    return
  }

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

  cliConf.save = cliConf.save || !cliConf.saveDev && !cliConf.saveOptional
  if (!cliConf['user-agent']) {
    cliConf['user-agent'] = `${pkg.name}/${pkg.version} npm/? node/${process.version} ${process.platform} ${process.arch}`
  }
  const force = cliConf.force === true
  // removing force to avoid redundant logs from npm
  // see issue #878 and #877
  delete cliConf.force

  await new Promise((resolve, reject) => {
    npm.load(cliConf as any, (err: Error) => { // tslint:disable-line
      if (err) {
        reject(err)
        return
      }
      resolve()
    })
  })

  const silent = npm.config.get('loglevel') === 'silent' || !npm.config.get('reporter') && isCI

  const opts = R.fromPairs(<any>R.keys(types).map(configKey => [camelcase(configKey), npm.config.get(configKey)])) // tslint:disable-line
  opts.rawNpmConfig = Object.assign.apply(Object, npm.config.list.reverse())
  opts.bin = npm.bin
  opts.globalBin = npm.globalBin
  opts.globalPrefix = path.join(npm.globalPrefix, 'pnpm-global')
  opts.prefix = opts.global ? opts.globalPrefix : npm.prefix
  opts.packageManager = pkg
  opts.force = force

  if (opts.only === 'prod' || opts.only === 'production' || !opts.only && opts.production) {
    opts.production = true
    opts.development = false
    opts.optional = true
  } else if (opts.only === 'dev' || opts.only === 'development') {
    opts.production = false
    opts.development = true
    opts.optional = false
  } else {
    opts.production = true
    opts.development = true
    opts.optional = true
  }

  initReporter(silent ? 'silent' : (<any>opts.reporter || 'default'), cmd) // tslint:disable-line
  delete opts.reporter // This is a silly workaround because supi expects a function as opts.reporter

  // `pnpm install ""` is going to be just `pnpm install`
  const cliArgs = cliConf.argv.remain.slice(1).filter(Boolean)
  return pnpmCmds[cmd](cliArgs, opts, cliConf.argv.remain[0])
}

export = run

import errorHandler from '../err'
if (!module.parent) run(process.argv.slice(2)).catch(errorHandler)
