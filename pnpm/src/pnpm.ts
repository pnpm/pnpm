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
    const { version } = (await import('@pnpm/cli-meta')).packageManager
    console.log(version)
    break
  }
  case 'completion': {
    const { getCompletionScript } = await import('@pnpm/tabtab')
    function exitError (message: string): never {
      console.error(message)
      process.exit(1)
    }
    const shell = argv[1]?.trim()
    if (!shell) {
      exitError('missing argument for shell')
    }
    if (!['bash', 'fish', 'pwsh', 'zsh'].includes(shell)) {
      exitError(`${shell} is not supported`)
    }
    const completionScript = await getCompletionScript({ name: 'pnpm', completer: 'pnpm', shell })
    console.log(completionScript)
    return
  }
  case 'install-completion': {
    const { install: installCompletion } = await import('@pnpm/tabtab')
    await installCompletion({ name: 'pnpm', completer: 'pnpm', shell: argv[1] })
    return
  }
  case 'uninstall-completion': {
    const { uninstall: uninstallCompletion } = await import('@pnpm/tabtab')
    await Promise.all(
      ['bash', 'fish', 'pwsh', 'zsh']
        .map((shell) => uninstallCompletion({ name: 'pnpm', shell }))
    )
    return
  }
  // commands that are passed through to npm:
  case 'access':
  case 'adduser':
  case 'bugs':
  case 'deprecate':
  case 'dist-tag':
  case 'docs':
  case 'edit':
  case 'info':
  case 'login':
  case 'logout':
  case 'owner':
  case 'ping':
  case 'prefix':
  case 'profile':
  case 'pkg':
  case 'repo':
  case 's':
  case 'se':
  case 'search':
  case 'set-script':
  case 'show':
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
  const { errorHandler } = await import('./errorHandler')
  try {
    const { main } = await import('./main')
    await main(argv)
  } catch (err: any) { // eslint-disable-line
    await errorHandler(err)
  }
}

async function passThruToNpm () {
  const { runNpm } = await import('./runNpm')
  const { status } = await runNpm(argv)
  process.exit(status!)
}
