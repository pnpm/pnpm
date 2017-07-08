import chalk = require('chalk')
import * as terminalWriter from './terminalWriter'
import {
  ProgressLog,
  LifecycleLog,
  Log,
  InstallCheckLog,
} from 'pnpm-logger'
import reportError from './reportError'
import os = require('os')

const EOL = os.EOL

const addedSign = chalk.green('+')
const removedSign = chalk.red('-')

export default function (streamParser: Object) {
  let resolutionDone = false
  let pkgsDiff: {name: string, version?: string, added: boolean}[] = []

  streamParser['on']('data', (obj: Log) => {
    switch (obj.name) {
      case 'pnpm:progress':
        reportProgress(<ProgressLog>obj)
        return
      case 'pnpm:stage':
        if (obj['message'] === 'resolution_done') {
          resolutionDone = true
          updateProgress()
        }
        return
      case 'pnpm:lifecycle':
        reportLifecycle(<LifecycleLog>obj)
        return
      case 'pnpm:install-check':
        reportInstallCheck(<InstallCheckLog>obj)
        return
      case 'pnpm:registry':
        if (obj.level === 'warn') {
          printWarn(obj['message'])
        }
        return
      case 'pnpm:root':
        if (obj['added']) {
          pkgsDiff.push({
            name: obj['added'].name,
            version: obj['added'].version,
            added: true,
          })
          return
        }
        if (obj['removed']) {
          pkgsDiff.push({
            name: obj['removed'].name,
            version: obj['removed'].version,
            added: false,
          })
          return
        }
        return
      case 'pnpm:summary':
        pkgsDiff.sort((a, b) => (a.name.localeCompare(b.name) + (Number(!b.added) - Number(!a.added))))
        const msg = pkgsDiff.map(pkg => {
          let result = pkg.added ? addedSign : removedSign
          result += ` ${pkg.name}`
          if (pkg.version) {
            result += ` ${chalk.grey(pkg.version)}`
          }
          return result
        }).join(EOL)
        if (!msg) return
        terminalWriter.write(`${EOL}${msg}`)
        return
      default:
        if (obj.level === 'debug') return
        if (obj.name !== 'pnpm' && obj.name.indexOf('pnpm:') !== 0) return
        if (obj.level === 'warn') {
          printWarn(obj['message'])
          return
        }
        if (obj.level === 'error') {
          reportError(obj)
          return
        }
        terminalWriter.write(obj['message'])
        return
    }
  })

  let resolving = 0
  let fetched = 0
  let foundInStore = 0

  function reportProgress (logObj: ProgressLog) {
    switch (logObj.status) {
      case 'resolving_content':
        resolving++
        break
      case 'found_in_store':
        foundInStore++;
        break
      case 'fetched':
        fetched++;
        break
      default:
        return
    }
    updateProgress()
  }

  function updateProgress() {
    const msg = `Resolving: total ${resolving}, reused ${foundInStore}, downloaded ${fetched}`
    if (resolving === foundInStore + fetched && resolutionDone) {
      terminalWriter.fixedWrite(`${msg}, done`)
      terminalWriter.done()
    } else {
      terminalWriter.fixedWrite(msg)
    }
  }
}

function reportLifecycle (logObj: LifecycleLog) {
  if (logObj.level === 'error') {
    terminalWriter.write(`${chalk.blue(logObj.pkgId)}! ${chalk.gray(logObj.line)}`)
    return
  }
  terminalWriter.write(`${chalk.blue(logObj.pkgId)}  ${chalk.gray(logObj.line)}`)
}

function reportInstallCheck (logObj: InstallCheckLog) {
  switch (logObj.code) {
    case 'EBADPLATFORM':
      printWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`)
      break
    case 'ENOTSUP':
      terminalWriter.write(logObj.toString())
      break
  }
}

function printWarn (message: string) {
  terminalWriter.write(`${chalk.yellow('WARN')} ${message}`)
}
