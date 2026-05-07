'use strict'
import path from 'node:path'

// Avoid "Possible EventEmitter memory leak detected" warnings
// because it breaks pnpm's CLI output
process.setMaxListeners(0)

const argv = buildArgv()

; (async () => {
  await runPnpm()
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

// Resolve `pnpx` / `pnx` aliases of the SEA binary. On Windows the @pnpm/exe
// install hardlinks pnpx.exe / pnx.exe to pnpm.exe; when invoked via either
// of those names, `process.execPath` reflects the launch path, so we read its
// basename and prepend `dlx` to argv. The non-SEA entry points (bin/pnpx.mjs,
// shell scripts in pnpm setup) inject `dlx` themselves before reaching this
// file, so this only triggers for the SEA path. See
// https://github.com/pnpm/pnpm/issues/11486.
function buildArgv (): string[] {
  const userArgv = process.argv.slice(2)
  const execName = path.basename(process.execPath, path.extname(process.execPath)).toLowerCase()
  if (execName === 'pnpx' || execName === 'pnx') {
    return ['dlx', ...userArgv]
  }
  return userArgv
}
