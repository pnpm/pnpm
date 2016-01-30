var chalk = require('chalk')

var observatory = require('observatory')
observatory.settings({ prefix: '  ', width: 74 })

module.exports = function logger () {
  return function (pkg) {
    var pkgData // package.json
    var res // resolution

    var t = observatory.add(pkg.name + ' ' +
      chalk.gray(pkg.rawSpec || ''))
      .status(chalk.yellow('·'))
    return status

    // log('resolved', pkgData)
    // log('downloading')
    // log('downloading', { done: 1, total: 200 })
    // log('depnedencies')
    // log('error', err)
    function status (status, args) {
      if (status === 'resolved') {
        res = args
      } else if (status === 'downloading') {
        if (res.version) {
          t.status(chalk.yellow('downloading ' + res.version + ' ·'))
        } else {
          t.status(chalk.yellow('downloading ·'))
        }
        if (args && args.total && args.done < args.total) {
          t.details('' + Math.round(args.done / args.total * 100) + '%')
        } else {
          t.details('')
        }
      } else if (status === 'done') {
        if (pkgData) {
          t.status(chalk.green('' + pkgData.version + ' ✓'))
            .details('')
        } else {
          t.status(chalk.green('OK ✓'))
            .details('')
        }
      } else if (status === 'package.json') {
        pkgData = args
      } else if (status === 'dependencies') {
        t.status(chalk.gray('' + pkgData.version + ' ·'))
          .details('')
      } else if (status === 'error') {
        t.status(chalk.red('ERROR ✗'))
          .details(args && (args.message || args))
      } else if (status === 'stdout') {
        observatory.add(chalk.blue(args.name) + '  ' + chalk.gray(args.line))
      } else if (status === 'stderr') {
        observatory.add(chalk.blue(args.name) + '! ' + chalk.gray(args.line))
      } else {
        t.status(status)
          .details('')
      }
    }
  }
}
