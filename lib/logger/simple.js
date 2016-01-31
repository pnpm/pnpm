var chalk = require('chalk')

var UPDATERS = [
  'resolving', 'resolved', 'download-start', 'dependencies'
]

var BAR_LENGTH = 20

var s = {
  gray: chalk.gray,
  green: chalk.green,
  bold: chalk.bold
}

/*
 * Simple percent logger
 */

module.exports = function logger () {
  var out = process.stdout
  var progress = { done: 0, total: 0 }
  var lastStatus
  var done = {}

  process.on('exit', _ => {
    out.write(reset())
  })

  return function (pkg) {
    var name = pkg.name
      ? (pkg.name + ' ' + pkg.rawSpec)
      : pkg.rawSpec

    update()
    progress.total += UPDATERS.length + 20
    var left = UPDATERS.length + 20
    var pkgData

    return function (status, args) {
      if (status === 'done') progress.done += left

      if (~UPDATERS.indexOf(status)) {
        progress.done += 1
        left -= 1
      }

      if (status === 'package.json') {
        pkgData = args
      }

      lastStatus = name

      if (process.env.VERBOSE) {
        if (status !== 'downloading') update(getName() + ' ' + status)
      } else if (status === 'done') {
        update(getName())
      } else {
        update()
      }
    }

    function getName () {
      if (pkgData && pkgData.version) {
        return pkgData.name + ' ' + s.gray(pkgData.version)
      } else {
        return pkgData && pkgData.name || name
      }
    }

    function update (line) {
      if (line && !done[line]) {
        done[line] = true
        out.write(reset() + line + '\n')
      }

      var percent = progress.done / progress.total
      if (progress.total > 0 && out.isTTY) {
        var bar = Math.round(percent * BAR_LENGTH)
        out.write(
          reset() +
          s.bold(Math.round(percent * 100) + '%') + ' ' +
          s.green(Array(bar).join('=') + '>') +
          Array(BAR_LENGTH - bar).join(' ') + ' ' +
          s.gray(lastStatus.substr(0, 40))) + ' '
      }
    }
  }

  function reset () {
    return out.isTTY
      ? '\r' + Array(out.columns).join(' ') + '\r'
      : ''
  }
}
