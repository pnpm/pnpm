// Avoid "Possible EventEmitter memory leak detected" warnings
// because it breaks pnpm's CLI output
process.setMaxListeners(0)

const argv = process.argv.slice(2)

// eslint-disable-next-line @typescript-eslint/no-floating-promises
; (async () => {
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
    // When the project's packageManager field selects pnpm v11 or newer, skip
    // the legacy npm passthrough: those pnpm versions implement these
    // commands natively, and passing through first would bypass main()'s
    // switchCliVersion and hand control to npm instead of the wanted pnpm.
    if (await shouldSkipNpmPassthrough()) {
      await runPnpm()
    } else {
      await passThruToNpm()
    }
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

async function shouldSkipNpmPassthrough (): Promise<boolean> {
  // Lazy-loaded so the extra fs/path work only happens on the passthrough
  // branch, preserving cold-start for everything else.
  const { shouldSkipNpmPassthrough: decide } = await import('./readWantedPnpmMajor.js')
  return decide(process.env as { COREPACK_ROOT?: string, npm_config_manage_package_manager_versions?: string })
}
