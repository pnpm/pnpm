'use strict'
import { isPnpxExecutable } from '@pnpm/cli.meta'

// Avoid "Possible EventEmitter memory leak detected" warnings
// because it breaks pnpm's CLI output
process.setMaxListeners(0)

const argv = buildArgv()

; (async () => {
  await runPnpm()
})()

async function runPnpm (): Promise<void> {
  const { finishWorkers } = await import('@pnpm/worker')
  const { errorHandler } = await import('./errorHandler.js')
  try {
    const { main } = await import('./main.js')
    await main(argv)
  } catch (err: any) { // eslint-disable-line
    await errorHandler(err)
  } finally {
    // `pnpm --version` short-circuits main() after switchCliVersion has
    // already spawned the worker pool — drain it here so the event loop
    // can exit. Swallow rejections so cleanup never masks the real result.
    try {
      await finishWorkers()
    } catch { /* ignore */ }
  }
}

// Resolve `pnpx` / `pnx` aliases of the SEA binary by prepending `dlx`. The
// non-SEA entry points (bin/pnpx.mjs, shell scripts in pnpm setup) inject
// `dlx` themselves before reaching this file, so this only triggers for the
// SEA path. See https://github.com/pnpm/pnpm/issues/11486.
function buildArgv (): string[] {
  const userArgv = process.argv.slice(2)
  if (isPnpxExecutable(process.execPath)) {
    return ['dlx', ...userArgv]
  }
  return userArgv
}
