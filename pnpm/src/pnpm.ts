import { readWantedPnpmMajor } from './readWantedPnpmMajor.js'

// Avoid "Possible EventEmitter memory leak detected" warnings
// because it breaks pnpm's CLI output
process.setMaxListeners(0)

const argv = process.argv.slice(2)

// eslint-disable-next-line @typescript-eslint/no-floating-promises
; (async () => {
  // When the project's packageManager field selects pnpm v11 or newer, skip
  // the legacy argv[0] npm passthrough: those pnpm versions implement the
  // commands natively, and passing through first would bypass main()'s
  // switchCliVersion and hand control to npm instead of the wanted pnpm.
  if (shouldSkipNpmPassthrough()) {
    await runPnpm()
    return
  }
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

function shouldSkipNpmPassthrough (): boolean {
  // Corepack already resolved which pnpm to run; don't second-guess it.
  if (process.env.COREPACK_ROOT != null) return false
  // A parent pnpm already switched to us via switchCliVersion — skipping the
  // passthrough now would loop right back into switchCliVersion again.
  if (process.env.npm_config_manage_package_manager_versions === 'false') return false
  const wantedMajor = readWantedPnpmMajor()
  return wantedMajor != null && wantedMajor >= 11
}
