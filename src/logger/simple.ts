import chalk = require('chalk')
import logger = require('@zkochan/logger')
import {PackageSpec} from '../resolve'

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

  const pkgDataMap = {}

  logger.on('progress', (pkg: PackageSpec, level: string, pkgSpec: string, status: string, args: Object) => {
    const name = pkg.name
      ? (pkg.name + ' ' + pkg.rawSpec)
      : pkg.rawSpec

    update()
    progress.total += UPDATERS.length + 20
    let left = UPDATERS.length + 20
    const pkgData = pkgDataMap[pkgSpec]

    if (status === 'done') progress.done += left

    if (~UPDATERS.indexOf(status)) {
      progress.done += 1
      left -= 1
    }

    if (status === 'package.json') {
      pkgDataMap[pkgSpec] = args
    }

    lastStatus = name

    if (process.env.VERBOSE) {
      if (status !== 'downloading') update(getName() + ' ' + status)
    } else if (status === 'done') {
      update(getName())
    } else {
      update()
    }

    function getName () {
      if (pkgData && pkgData.version) {
        return pkgData.name + ' ' + s.gray(pkgData.version)
      } else {
        return pkgData && pkgData.name || name
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
  })

  function reset () {
    return out['isTTY']
      ? '\r' + Array(out['columns']).join(' ') + '\r'
      : ''
  }
}
