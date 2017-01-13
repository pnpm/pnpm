import chalk = require('chalk')
import observatory = require('observatory')
import {ProgressLog, DownloadStatus} from '../logging/logInstallStatus'
import streamParser from '../logging/streamParser'

observatory.settings({ prefix: '  ', width: 74 })

export default function () {
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

  streamParser.on('data', (obj: ProgressLog) => {
    if (obj['name'] !== 'progress') return
    logProgress(obj)
  })

  function logProgress (logObj: ProgressLog) {
    // lazy get task
    function t () {
      return getTask(logObj.pkg.rawSpec, logObj.pkg.name)
    }

    // the first thing it (probably) does is wait in queue to query the npm registry

    switch (logObj.status) {
      case 'resolving':
        t().status(chalk.yellow('finding ·'))
        return
      case 'download-queued':
        if (logObj.pkg.version) {
          t().status(chalk.gray('queued ' + logObj.pkg.version + ' ↓'))
          return
        }
        t().status(chalk.gray('queued ↓'))
        return
      case 'downloading':
      case 'download-start':
        if (logObj.pkg.version) {
          t().status(chalk.yellow('downloading ' + logObj.pkg.version + ' ↓'))
        } else {
          t().status(chalk.yellow('downloading ↓'))
        }
        if (logObj.downloadStatus && logObj.downloadStatus.total && logObj.downloadStatus.done < logObj.downloadStatus.total) {
          t().details('' + Math.round(logObj.downloadStatus.done / logObj.downloadStatus.total * 100) + '%')
        } else {
          t().details('')
        }
        return
      case 'done':
        if (logObj.pkg.version) {
          t().status(chalk.green('' + logObj.pkg.version + ' ✓'))
            .details('')
          return
        }
        t().status(chalk.green('OK ✓')).details('')
        return
      case 'dependencies':
        t().status(chalk.gray('' + logObj.pkg.version + ' ·'))
          .details('')
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
