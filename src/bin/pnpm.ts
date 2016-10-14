#!/usr/bin/env node
// NOTE: This should be done as soon as possible because the debug
// package reads the env variable only once
if (~process.argv.indexOf('--debug')) {
  process.env.DEBUG = 'pnpm:*'
  process.argv.push('--silent')
}

import loudRejection = require('loud-rejection')
loudRejection()
import rc = require('rc')
import meow = require('meow')
import updateNotifier = require('update-notifier')
import camelcaseKeys = require('camelcase-keys')
import isCI = require('is-ci')
import {stripIndent} from 'common-tags'
import '../fileLogger'
import pkg from '../pnpmPkgJson'
import runNpm from '../cmd/runNpm'
import installCmd from '../cmd/install'
import uninstallCmd from '../cmd/uninstall'
import linkCmd from '../cmd/link'
import publishCmd from '../cmd/publish'
import pruneCmd from '../cmd/prune'
import installTestCmd from '../cmd/installTest'
import cacheCmd from '../cmd/cache'

const pnpmCmds = {
  install: installCmd,
  uninstall: uninstallCmd,
  link: linkCmd,
  publish: publishCmd,
  prune: pruneCmd,
  'install-test': installTestCmd,
  cache: cacheCmd,
}

const supportedCmds = new Set([
  'install',
  'uninstall',
  'help',
  'link',
  'publish',
  'prune',
  'install-test',
  'cache',
])

function run (argv: string[]) {
  const cli = meow({
    argv: argv,
    help: stripIndent`
      Usage:
        $ pnpm install
        $ pnpm install <name>
        $ pnpm uninstall
        $ pnpm uninstall <name>

      Options:
        -S, --save            save into package.json under dependencies
        -D, --save-dev        save into package.json under devDependencies
        -O, --save-optional   save into package.json under optionalDependencies
        -E, --save-exact      save exact spec

            --dry-run         simulate
        -g, --global          install globally

            --production      don't install devDependencies
            --silent           don't print progress
            --debug           print verbose debug message`
  }, {
    boolean: [
      'save-dev', 'save', 'save-exact', 'save-optional', 'dry-run', 'global', 'silent', 'debug'
    ],
    alias: {
      quiet: 'silent',
      'no-progress': 'silent',
      D: 'save-dev',
      S: 'save',
      E: 'save-exact',
      O: 'save-optional',
      g: 'global',
      v: 'version'
    }
  })

  if (!isCI) {
    updateNotifier({pkg}).notify()
  }

  const cmd = getCommandFullName(cli.input[0])
  if (!supportedCmds.has(cmd)) {
    runNpm(argv)
    return Promise.resolve()
  }

  cli.flags.silent = cli.flags.silent || cli.flags.debug || isCI

  ; ['dryRun'].forEach(flag => {
    if (cli.flags[flag]) {
      console.error(`Error: '${flag}' is not supported yet, sorry!`)
      process.exit(1)
    }
  })

  const opts = Object.assign({}, getRC('npm'), getRC('pnpm'))

  // This is needed because the arg values should be used only if they were passed
  Object.keys(cli.flags).forEach(key => {
    opts[key] = opts[key] || cli.flags[key]
  })

  const cliArgs = cli.input.slice(1)
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
    case 'help':
      return 'help'
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

function getRC (appName: string) {
  return camelcaseKeys(rc(appName))
}

export default run

import errorHandler from '../err'
if (!module.parent) run(process.argv.slice(2)).catch(errorHandler)
