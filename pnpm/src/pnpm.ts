'use strict'
// Avoid "Possible EventEmitter memory leak detected" warnings
// because it breaks pnpm's CLI output
process.setMaxListeners(0)

const argv = process.argv.slice(2)


  ; (async () => {
  const { switchNodeBasedOnDevEngine } = await import('./switchNodeBasedOnDevEngine.js');
  if (await switchNodeBasedOnDevEngine()) return;

  switch (argv[0]) {
  // commands that are passed through to npm:
  case 'access':
  case 'adduser':
  case 'bugs':
  case 'deprecate':
  case 'dist-tag':
  case 'docs':
  case 'edit':
  case 'find':
  case 'home':
  case 'info':
  case 'issues':
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

async function runPnpm (): Promise<void> {
  const { errorHandler } = await import('./errorHandler.js')
  try {
    const { main } = await import('./main.js')
    await main(argv)
  } catch (err: any) { // eslint-disable-line
    await errorHandler(err)
  }
}

async function passThruToNpm (): Promise<void> {
  const { runNpm } = await import('./runNpm.js')
  const { status } = await runNpm(argv)
  process.exit(status!)
}
