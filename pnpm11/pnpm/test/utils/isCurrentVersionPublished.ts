import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { pnpmBinLocation } from './execPnpm.js'

/**
 * Tells whether the pnpm version under test is already published to npmjs as
 * both the `pnpm` and `@pnpm/exe` packages. Tests that resolve the running
 * version through the registry can only pass once the release is published,
 * so they must be skipped in the window between a release commit landing on
 * main and the matching npm publish. Also returns `false` when npmjs cannot
 * be reached, since such tests would fail on the registry request anyway.
 * Emits a warning when it returns `false`, so skips are visible in the logs.
 */
export function isCurrentVersionPublished (): boolean {
  const manifestPath = path.join(path.dirname(pnpmBinLocation), '..', 'package.json')
  const { version } = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as { version: string }
  const urls = ['pnpm', '@pnpm/exe'].map(name => `https://registry.npmjs.org/${name}/${version}`)
  const { status } = spawnSync(process.execPath, ['-e', ALL_URLS_OK_SCRIPT, ...urls], { timeout: 60_000 })
  if (status === 0) return true
  console.warn(`Version ${version} of pnpm and/or @pnpm/exe is not available on registry.npmjs.org yet. Tests that resolve the running version from the registry will be skipped.`)
  return false
}

// process.argv[0] is the node binary when running via `node -e`, so the URLs
// start at index 1.
const ALL_URLS_OK_SCRIPT = `
Promise.all(process.argv.slice(1).map((url) => fetch(url).then((response) => response.ok)))
  .then((allOk) => {
    process.exitCode = allOk.every(Boolean) ? 0 : 1
  }, () => {
    process.exitCode = 1
  })
`
