'use strict'
let argv = process.argv.slice(2)

if (argv.indexOf('--help') !== -1 || argv.indexOf('-h') !== -1 || argv.indexOf('--h') !== -1) {
  argv = ['help'].concat(argv)
}

(async () => { // tslint:disable-line:no-floating-promises
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
    // commands that are passed through to npm:
    case 'access':
    case 'adduser':
    case 'bin':
    case 'bugs':
    case 'c':
    case 'config':
    case 'deprecate':
    case 'dist-tag':
    case 'docs':
    case 'edit':
    case 'get':
    case 'info':
    case 'init':
    case 'login':
    case 'logout':
    case 'owner':
    case 'pack':
    case 'ping':
    case 'prefix':
    case 'profile':
    case 'publish':
    case 'repo':
    case 'restart':
    case 's':
    case 'se':
    case 'search':
    case 'set':
    case 'star':
    case 'stars':
    case 'start':
    case 'stop':
    case 'team':
    case 't':
    case 'tst':
    case 'test':
    case 'token':
    case 'unpublish':
    case 'unstar':
    case 'v':
    case 'version':
    case 'view':
    case 'whoami':
    case 'xmas':
    case 'run':
    case 'run-script':
      const runNpm = (await import('../cmd/runNpm')).default
      runNpm(argv)
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
