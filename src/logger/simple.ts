import chalk = require('chalk')
import streamParser from './streamParser'
import {PackageSpec} from '../resolve'
import {ProgressLog, DownloadStatus} from './logInstallStatus'

const UPDATERS = [
  'resolving', 'resolved', 'download-start', 'dependencies'
]

const BAR_LENGTH = 20

const s = {
  gray: chalk.gray,
  green: chalk.green,
  bold: chalk.bold
}

/**
 * Simple percent logger
 */
export default function () {
  const out = process.stdout
  const progress = { done: 0, total: 0 }
  let lastStatus: string
  const done = {}

  process.on('exit', () => {
    out.write(reset())
  })

  streamParser.on('data', (obj: ProgressLog) => {
    if (obj['name'] !== 'progress') return
    logProgress(obj)
  })

  function logProgress (logObj: ProgressLog) {
    const name = logObj.pkg.name
      ? (logObj.pkg.name + ' ' + logObj.pkg.rawSpec)
      : logObj.pkg.rawSpec

    update()
    progress.total += UPDATERS.length + 20
    let left = UPDATERS.length + 20

    if (logObj.status === 'done') progress.done += left

    if (~UPDATERS.indexOf(logObj.status)) {
      progress.done += 1
      left -= 1
    }

    lastStatus = name

    if (process.env.VERBOSE) {
      if (logObj.status !== 'downloading') update(getName() + ' ' + logObj.status)
    } else if (logObj.status === 'done') {
      update(getName())
    } else {
      update()
    }

    function getName () {
      if (logObj.pkg && logObj.pkg.version) {
        return logObj.pkg.name + ' ' + s.gray(logObj.pkg.version)
      } else {
        return logObj.pkg && logObj.pkg.name || name
      }
    }

    function update (line?: string) {
      if (line && !done[line]) {
        done[line] = true
        out.write(reset() + line + '\n')
      }

      const percent = progress.done / progress.total
      if (progress.total > 0 && out['isTTY']) {
        const bar = Math.round(percent * BAR_LENGTH)
        out.write(
          reset() +
          s.bold(Math.round(percent * 100) + '%') + ' ' +
          s.green(Array(bar).join('=') + '>') +
          Array(BAR_LENGTH - bar).join(' ') + ' ' +
          s.gray(lastStatus.substr(0, 40)))
      }
    }
  }

  function reset () {
    return out['isTTY']
      ? '\r' + Array(out['columns']).join(' ') + '\r'
      : ''
  }
}
