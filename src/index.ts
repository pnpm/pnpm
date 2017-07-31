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

type PackageDiff = {
  name: string,
  version?: string,
  added: boolean,
  deprecated?: boolean,
}

const propertyByDependencyType = {
  prod: 'dependencies',
  dev: 'devDependencies',
  optional: 'optionalDependencies',
}

export default function (streamParser: Object) {
  let resolutionDone = false
  let pkgsDiff: {
    prod: PackageDiff[],
    dev: PackageDiff[],
    optional: PackageDiff[],
  } = {
    prod: [],
    dev: [],
    optional: [],
  }
  const deprecated = {}

  streamParser['on']('data', (obj: Log) => {
    switch (obj.name) {
      case 'pnpm:progress':
        reportProgress(obj)
        return
      case 'pnpm:stage':
        if (obj.message === 'resolution_done') {
          resolutionDone = true
          updateProgress()
        }
        return
      case 'pnpm:lifecycle':
        reportLifecycle(obj)
        return
      case 'pnpm:install-check':
        reportInstallCheck(obj)
        return
      case 'pnpm:registry':
        if (obj.level === 'warn') {
          printWarn(obj.message)
        }
        return
      case 'pnpm:root':
        if (obj['added']) {
          pkgsDiff[obj['added'].dependencyType].push({
            name: obj['added'].name,
            version: obj['added'].version,
            deprecated: !!deprecated[obj['added'].id],
            added: true,
          })
          return
        }
        if (obj['removed']) {
          pkgsDiff[obj['removed'].dependencyType].push({
            name: obj['removed'].name,
            version: obj['removed'].version,
            added: false,
          })
          return
        }
        return
      case 'pnpm:summary':
        let msg = ''
        for (const depType of ['prod', 'optional', 'dev']) {
          if (pkgsDiff[depType].length) {
            msg += EOL
            msg += chalk.blue(`${propertyByDependencyType[depType]}:`)
            msg += EOL
            msg += printDiffs(pkgsDiff[depType])
            msg += EOL
          }
        }
        if (!msg) return
        terminalWriter.write(msg)
        return
      case 'pnpm:deprecation':
        // print warnings only about deprecated packages from the root
        if (obj.depth > 0) return
        deprecated[obj.pkgId] = obj.deprecated
        printWarn(`${chalk.red('deprecated')} ${obj.pkgName}@${obj.pkgVersion}: ${obj.deprecated}`)
        return
      case 'pnpm':
        if (obj.level === 'debug') return
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

function printDiffs (pkgsDiff: PackageDiff[]) {
  // Sorts by alphabet then by removed/added
  // + ava 0.10.0
  // - chalk 1.0.0
  // + chalk 2.0.0
  pkgsDiff.sort((a, b) => (a.name.localeCompare(b.name) * 10 + (Number(!b.added) - Number(!a.added))))
  const msg = pkgsDiff.map(pkg => {
    let result = pkg.added ? addedSign : removedSign
    result += ` ${pkg.name}`
    if (pkg.version) {
      result += ` ${chalk.grey(pkg.version)}`
    }
    if (pkg.deprecated) {
      result += ` ${chalk.red('deprecated')}`
    }
    return result
  }).join(EOL)
  return msg
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
