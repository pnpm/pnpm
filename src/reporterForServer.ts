import chalk from 'chalk'
import os = require('os')
import prettyBytes = require('pretty-bytes')
import R = require('ramda')
import semver = require('semver')
import {
  DeprecationLog,
  InstallCheckLog,
  LifecycleLog,
  Log,
  ProgressLog,
  RegistryLog,
} from 'supi'
import xs, {Stream} from 'xstream'
import getPkgsDiff, {
  PackageDiff,
  propertyByDependencyType,
} from './pkgsDiff'
import reportError from './reportError'

const EOL = os.EOL

export default function (
  log$: xs<Log>,
) {
  const outputs: Array<xs<xs<{msg: string}>>> = []

  log$.addListener({
    next (log) {
      if (log.name === 'pnpm:progress') {
        switch (log.status) {
          case 'fetched':
          case 'fetching_started':
            console.log(`${chalk.cyan(log.status)} ${log.pkgId}`)
        }
        return
      }
      switch (log.level) {
        case 'warn':
          console.log(formatWarn(log['message']))
          return
        case 'error':
          console.log(reportError(log))
          return
        case 'debug':
          return
        default:
          console.log(log['message'])
          return
      }
    },
  })
}

function formatWarn (message: string) {
  // The \u2009 is the "thin space" unicode character
  // It is used instead of ' ' because chalk (as of version 2.1.0)
  // trims whitespace at the beginning
  return `${chalk.bgYellow.black('\u2009WARN\u2009')} ${message}`
}
