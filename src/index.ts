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
    const taskId = `${pkgName}/${pkgRawSpec}`
    if (tasks[taskId]) return tasks[taskId]
    const task = observatory.add(
      (pkgName ? (pkgName + ' ') : '') +
      chalk.gray(pkgRawSpec || ''))
    task.status(chalk.gray('·'))
    tasks[taskId] = task
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
        observatory.add(obj['message'])
        return
    }
  })

  function reportProgress (logObj: ProgressLog) {
    // lazy get task
    function t () {
      return getTask(logObj.pkg.rawSpec, logObj.pkg.name)
    }

    if (logObj.pkg.dependentId) return

    // the first thing it (probably) does is wait in queue to query the npm registry

    switch (logObj.status) {
      case 'installing':
        t().status(chalk.gray('queued ↓'))
        return
      case 'resolving':
        t().status(chalk.yellow('finding ·'))
        return
      case 'resolved':
        if (logObj.pkg.version) {
          t().status(chalk.yellow('installing ' + logObj.pkg.version + ' .'))
          return
        }
        t().status(chalk.yellow('installing .'))
        return
      case 'fetched':
        if (logObj.pkg.version) {
          t().status(chalk.yellow('installing dependencies ' + logObj.pkg.version + ' .'))
          return
        }
        t().status(chalk.yellow('installing dependencies .'))
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
    observatory.add(`${chalk.blue(logObj.pkgId)}! ${chalk.gray(logObj.line)}`)
    return
  }
  observatory.add(`${chalk.blue(logObj.pkgId)}  ${chalk.gray(logObj.line)}`)
}

function reportInstallCheck (logObj: InstallCheckLog) {
  switch (logObj.code) {
    case 'EBADPLATFORM':
      printWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`)
      break
    case 'ENOTSUP':
      observatory.add(logObj)
      break
  }
}

function printWarn (message: string) {
  observatory.add(`${chalk.yellow('WARN')} ${message}`)
}
