#!/usr/bin/env node
// Patch the global fs module here at the app level
import '../fs/gracefulify'

import loudRejection = require('loud-rejection')
loudRejection()
import path = require('path')
import updateNotifier = require('update-notifier')
import camelcase = require('camelcase')
import isCI = require('is-ci')
import {stripIndent} from 'common-tags'
import nopt = require('nopt')
import R = require('ramda')
import npm = require('npm')
import npmDefaults = require('npm/lib/config/defaults')
import '../logging/fileLogger'
import pkg from '../pnpmPkgJson'
import runNpm from '../cmd/runNpm'
import installCmd from '../cmd/install'
import updateCmd from '../cmd/update'
import uninstallCmd from '../cmd/uninstall'
import linkCmd from '../cmd/link'
import pruneCmd from '../cmd/prune'
import installTestCmd from '../cmd/installTest'
import runCmd from '../cmd/run'
import storeCmd from '../cmd/store'
import bole = require('bole')
import initReporter from '../reporter'

bole.setFastTime()

const pnpmCmds = {
  install: installCmd,
  update: updateCmd,
  uninstall: uninstallCmd,
  link: linkCmd,
  prune: pruneCmd,
  'install-test': installTestCmd,
  run: runCmd,
  store: storeCmd,
}

const supportedCmds = new Set([
  'install',
  'uninstall',
  'update',
  'link',
  'prune',
  'install-test',
  'run',
  'store',
])

async function run (argv: string[]) {
  const pnpmTypes = {
    'store-path': path,
    'local-registry': path,
    'global-path': path,
    'network-concurrency': Number,
    'fetching-concurrency': Number,
    'lock-stale-duration': Number,
    'child-concurrency': Number,
    'offline': Boolean,
    'reporter': String,
  }
  const types = R.merge(npmDefaults.types, pnpmTypes)
  const cliConf = nopt(
    types,
    npmDefaults.shorthands,
    argv,
    0 // argv should be already sliced by now
  )

  argv = cliConf.argv.remain

  if (cliConf.version) {
    console.log(pkg.version)
    return
  }

  if (!isCI) {
    updateNotifier({
      packageName: pkg.name,
      packageVersion: pkg.version
    }).notify()
  }

  const cmd = getCommandFullName(argv[0])
  if (!supportedCmds.has(cmd)) {
    runNpm(argv)
    return Promise.resolve()
  }

  if (cliConf['dry-run']) {
    console.error(`Error: 'dry-run' is not supported yet, sorry!`)
    process.exit(1)
  }

  cliConf.save = cliConf.save || !cliConf.saveDev && !cliConf.saveOptional

  await new Promise((resolve, reject) => {
    npm.load(cliConf, (err: Error) => {
      if (err) {
        reject(err)
        return
      }
      resolve()
    })
  })

  const silent = npm.config.get('loglevel') === 'silent' || !npm.config.get('reporter') && isCI

  const opts = R.fromPairs(<any>R.keys(types).map(configKey => [camelcase(configKey), npm.config.get(configKey)])) // tslint:disable-line
  opts.rawNpmConfig = Object.assign.apply(Object, npm.config['list'].reverse())
  opts.bin = npm.bin
  opts.globalPrefix = path.join(npm['globalPrefix'], 'pnpm-global')
  opts.prefix = opts.global ? opts.globalPrefix : npm.prefix

  initReporter(silent ? 'silent' : (<any>opts.reporter || 'default')) // tslint:disable-line

  const cliArgs = argv.slice(1)
  return pnpmCmds[cmd](cliArgs, opts)
}

function getCommandFullName (cmd: string) {
  switch (cmd) {
    case 'install':
    case 'i':
      return 'install'
    case 'uninstall':
    case 'r':
    case 'rm':
    case 'un':
    case 'unlink':
      return 'uninstall'
    case 'link':
    case 'ln':
      return 'link'
    case 'install-test':
    case 'it':
      return 'install-test'
    // some commands have no aliases: publish, prune
    default:
      return cmd
  }
}

export default run

import errorHandler from '../err'
if (!module.parent) run(process.argv.slice(2)).catch(errorHandler)
