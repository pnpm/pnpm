#!/usr/bin/env node
if (~process.argv.indexOf('--debug')) {
  process.env.DEBUG = 'pnpm:*'
  process.argv.push('--quiet')
}

var rc = require('rc')
var camelcaseKeys = require('camelcase-keys')
var spawnSync = require('cross-spawn').sync

var installCmd = require('../lib/cmd/install')
var uninstallCmd = require('../lib/cmd/uninstall')

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

  var installCmds = ['install', 'i']
  var supportedCmds = installCmds.concat(['uninstall', 'r', 'rm', 'un', 'unlink', 'help'])
  if (supportedCmds.indexOf(cli.input[0]) === -1) {
    spawnSync('npm', argv, { stdio: 'inherit' })
    return
  }

  if (cli.flags.debug) {
    cli.flags.quiet = true
  }

  ['dryRun', 'global'].forEach(function (flag) {
    if (cli.flags[flag]) {
      console.error("Error: '" + flag + "' is not supported yet, sorry!")
      process.exit(1)
    }
  })

  var opts = Object.assign({}, getRC('npm'), getRC('pnpm'))

  // This is needed because the arg values should be used only if they were passed
  Object.keys(cli.flags).forEach(key => {
    opts[key] = opts[key] || cli.flags[key]
  })

  var cmd = installCmds.indexOf(cli.input[0]) === -1 ? uninstallCmd : installCmd
  return cmd(cli.input.slice(1), opts).catch(require('../lib/err'))
}

function getRC (appName) {
  return camelcaseKeys(rc(appName))
}

module.exports = run
if (!module.parent) run(process.argv.slice(2))
