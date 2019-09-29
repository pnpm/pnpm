'use strict'
// Avoid "Possible EventEmitter memory leak detected" warnings
// because it breaks pnpm's CLI output
process.setMaxListeners(0)

let argv = process.argv.slice(2)

const dashDashIndex = argv.indexOf('--')
const nonEscapedArgv = dashDashIndex === -1 ? argv : argv.slice(0, dashDashIndex)
const helpOptions = new Set(['--help', '-h', '--h'])

if (nonEscapedArgv.some((arg) => helpOptions.has(arg))) {
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
      help(argv.filter((arg) => !helpOptions.has(arg)).slice(1))
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
    case 'ping':
    case 'prefix':
    case 'profile':
    case 'repo':
    case 's':
    case 'se':
    case 'search':
    case 'set':
    case 'star':
    case 'stars':
    case 'team':
    case 'token':
    case 'unpublish':
    case 'unstar':
    case 'v':
    case 'version':
    case 'view':
    case 'whoami':
    case 'xmas':
      await passThruToNpm()
      break
    default:
      await runPnpm()
      break
  }
})()

async function runPnpm () {
  const errorHandler = (await import('../err')).default
  try {
    const main = (await import('../main')).default
    await main(argv)
  } catch (err) {
    errorHandler(err)
  }
}

async function passThruToNpm () {
  const runNpm = (await import('../cmd/runNpm')).default
  const { status } = runNpm(argv)
  process.exit(status)
}
