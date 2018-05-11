#!/usr/bin/env node
'use strict'
let argv = process.argv.slice(2)

if (argv.indexOf('--help') !== -1 || argv.indexOf('-h') !== -1 || argv.indexOf('--h') !== -1) {
  argv = ['help'].concat(argv)
}

(async () => {
  switch (argv[0]) {
    case '-v':
    case '--version':
      const pkg = (await import('../pnpmPkgJson')).default
      console.log(pkg.version)
      break
    case 'help':
      const help = (await import('../cmd/help')).default
      help(argv.slice(1))
      break
    default:
      const errorHandler = (await import('../err')).default
      try {
        const main = (await import('../main')).default
        await main(argv)
      } catch (err) {
        errorHandler(err)
      }
  }
})()
