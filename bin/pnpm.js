#!/usr/bin/env node
'use strict'
const rc = require('rc')
const camelcaseKeys = require('camelcase-keys')
const spawnSync = require('cross-spawn').sync

const pnpmCmds = {
  install: require('../lib/cmd/install'),
  uninstall: require('../lib/cmd/uninstall'),
  link: require('../lib/cmd/link')
}

const supportedCmds = new Set(['install', 'uninstall', 'help', 'link'])

function run (argv) {
  const cli = require('meow')({
    argv: argv,
    help: [
      'Usage:',
      '  $ pnpm install',
      '  $ pnpm install <name>',
      '  $ pnpm uninstall',
      '  $ pnpm uninstall <name>',
      '',
      'Options:',
      '  -S, --save            save into package.json under dependencies',
      '  -D, --save-dev        save into package.json under devDependencies',
      '  -O, --save-optional   save into package.json under optionalDependencies',
      '  -E, --save-exact      save exact spec',
      '',
      '      --dry-run         simulate',
      '  -g, --global          install globally',
      '',
      '      --production      don\'t install devDependencies',
      '      --quiet           don\'t print progress',
      '      --debug           print verbose debug message'
    ].join('\n')
  }, {
    boolean: [
      'save-dev', 'save', 'save-exact', 'save-optional', 'dry-run', 'global', 'quiet', 'debug'
    ],
    alias: {
      'no-progress': 'quiet',
      D: 'save-dev',
      S: 'save',
      E: 'save-exact',
      O: 'save-optional',
      g: 'global',
      v: 'version'
    }
  })

  const cmd = getCommandFullName(cli.input[0])
  if (!supportedCmds.has(cmd)) {
    spawnSync('npm', argv, { stdio: 'inherit' })
    return Promise.resolve()
  }

  if (cli.flags.debug) {
    process.env.DEBUG = 'pnpm:*'
    cli.flags.quiet = true
  }

  ['dryRun'].forEach(flag => {
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

function getCommandFullName (cmd) {
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
    default:
      return cmd
  }
}

function getRC (appName) {
  return camelcaseKeys(rc(appName))
}

module.exports = run
if (!module.parent) run(process.argv.slice(2)).catch(require('../lib/err'))
