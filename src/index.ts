import chalk = require('chalk')
import observatory = require('observatory')
import {
  ProgressLog,
  LifecycleLog,
  Log,
  InstallCheckLog,
} from 'pnpm-logger'
import reportError from './reportError'

observatory.settings({ prefix: '  ', width: 74 })

export default function (streamParser: Object) {
  const tasks = {}

  function getTask (pkgRawSpec: string, pkgName: string) {
    if (tasks[pkgRawSpec]) return tasks[pkgRawSpec]
    const task = observatory.add(
      (pkgName ? (pkgName + ' ') : '') +
      chalk.gray(pkgRawSpec || ''))
    task.status(chalk.gray('·'))
    tasks[pkgRawSpec] = task
    return task
  }

  streamParser['on']('data', (obj: Log) => {
    switch (obj.name) {
      case 'pnpm:progress':
        reportProgress(<ProgressLog>obj)
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
        console.log(obj['message'])
        return
    }
  })

  const trackedSpecs: string[] = []

  function reportProgress (logObj: ProgressLog) {
    // lazy get task
    function t () {
      return getTask(logObj.pkg.rawSpec, logObj.pkg.name)
    }

    if (logObj.status === 'installing') {
      if (!logObj.keypath || logObj.keypath.length > 0) {
        return
      }
      t().status(logObj.status)
        .details('')
      trackedSpecs.push(logObj.pkg.rawSpec)
      return
    }

    if (trackedSpecs.indexOf(logObj.pkg.rawSpec) === -1) return

    // the first thing it (probably) does is wait in queue to query the npm registry

    switch (logObj.status) {
      case 'resolving':
        t().status(chalk.yellow('finding ·'))
        return
      case 'resolved':
        if (logObj.pkg.version) {
          t().status(chalk.gray('queued ' + logObj.pkg.version + ' ↓'))
          return
        }
        t().status(chalk.gray('queued ↓'))
        return
      case 'fetching':
        if (logObj.pkg.version) {
          t().status(chalk.yellow('downloading ' + logObj.pkg.version + ' ↓'))
        } else {
          t().status(chalk.yellow('downloading ↓'))
        }
        if (logObj.progress && logObj.progress.total && logObj.progress.done < logObj.progress.total) {
          t().details('' + Math.round(logObj.progress.done / logObj.progress.total * 100) + '%')
        } else {
          t().details('')
        }
        return
      case 'installed':
        if (logObj.pkg.version) {
          t().status(chalk.green('' + logObj.pkg.version + ' ✓'))
            .details('')
          return
        }
        t().status(chalk.green('OK ✓')).details('')
        return
      case 'error':
        t().status(chalk.red('ERROR ✗'))
          .details('')
        return
      default:
        t().status(logObj.status)
          .details('')
        return
    }
  }
}

function reportLifecycle (logObj: LifecycleLog) {
  if (logObj.level === 'error') {
    console.log(chalk.blue(logObj.pkgId) + '! ' + chalk.gray(logObj.line))
    return
  }
  console.log(chalk.blue(logObj.pkgId) + '  ' + chalk.gray(logObj.line))
}

function reportInstallCheck (logObj: InstallCheckLog) {
  switch (logObj.code) {
    case 'EBADPLATFORM':
      printWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`)
      break
    case 'ENOTSUP':
      console.warn(logObj)
      break
  }
}

function printWarn (message: string) {
  console.log(chalk.yellow('WARN'), message)
}
