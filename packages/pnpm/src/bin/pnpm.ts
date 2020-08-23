'use strict'
// Avoid "Possible EventEmitter memory leak detected" warnings
// because it breaks pnpm's CLI output
process.setMaxListeners(0)

const argv = process.argv.slice(2)

// eslint-disable-next-line @typescript-eslint/no-floating-promises
; (async () => {
  switch (argv[0]) {
  case '-v':
  case '--version': {
    const { version } = (await import('@pnpm/cli-meta')).default
    console.log(version)
    break
  }
  case 'install-completion': {
    const { install: installCompletion } = await import('@pnpm/tabtab')
    await installCompletion({ name: 'pnpm', completer: 'pnpm' })
    return
  }
  case 'uninstall-completion': {
    const { uninstall: uninstallCompletion } = await import('@pnpm/tabtab')
    await uninstallCompletion({ name: 'pnpm' })
    return
  }
  // commands that are passed through to npm:
  case 'access':
  case 'adduser':
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
  const runNpm = (await import('../runNpm')).default
  const { status } = await runNpm(argv)
  process.exit(status!)
}
