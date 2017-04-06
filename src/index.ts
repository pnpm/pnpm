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

  const taskByPkgId = {}
  const pkgVersionByPkgId = {}

  function reportProgress (logObj: ProgressLog) {
    let task
    let pkgVersion = pkgVersionByPkgId[logObj['pkgId']]
    switch (logObj.status) {
      case 'fetching':
      case 'fetched':
      case 'installed':
      case 'dependencies_installed':
        task = taskByPkgId[logObj.pkgId]
        break
      case 'resolving':
      case 'error':
      case 'installing':
      case 'resolved':
        // log statuses of the top dependencies only
        if (logObj.pkg && logObj.pkg.dependentId) break

        task = getTask(logObj.pkg.rawSpec, logObj.pkg.name)

        if (logObj.status === 'resolved' && logObj.pkgId) {
          taskByPkgId[logObj.pkgId] = task
        }
        break
      default:
        if (logObj.status === 'downloaded_manifest') {
          pkgVersionByPkgId[logObj.pkgId] = logObj.pkgVersion
        }
        return
    }

    if (!task) return

    switch (logObj.status) {
      case 'installing':
        task.status(chalk.gray('queued ↓'))
        return
      case 'resolving':
        task.status(chalk.yellow('finding ·'))
        return
      case 'resolved':
        if (pkgVersion) {
          task.status(chalk.yellow('installing ' + pkgVersion + ' .'))
          return
        }
        task.status(chalk.yellow('installing .'))
        return
      case 'fetched':
        if (pkgVersion) {
          task.status(chalk.yellow('installing dependencies ' + pkgVersion + ' .'))
          return
        }
        task.status(chalk.yellow('installing dependencies .'))
        return
      case 'fetching':
        if (pkgVersion) {
          task.status(chalk.yellow('downloading ' + pkgVersion + ' ↓'))
        } else {
          task.status(chalk.yellow('downloading ↓'))
        }
        if (logObj.progress && logObj.progress.total && logObj.progress.done < logObj.progress.total) {
          task.details('' + Math.round(logObj.progress.done / logObj.progress.total * 100) + '%')
        } else {
          task.details('')
        }
        return
      case 'dependencies_installed':
        if (pkgVersion) {
          task.status(chalk.yellow('linking ' + pkgVersion + ' ↓'))
          return
        }
        task.status(chalk.yellow('linking ↓'))
        return
      case 'installed':
        if (pkgVersion) {
          task.status(chalk.green('' + pkgVersion + ' ✓'))
            .details('')
          return
        }
        task.status(chalk.green('OK ✓')).details('')
        return
      case 'error':
        task.status(chalk.red('ERROR ✗'))
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
