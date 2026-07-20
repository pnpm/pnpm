import { execFileSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import util from 'node:util'

import { test } from '@jest/globals'

/**
 * Regression guard for https://github.com/pnpm/pnpm/issues/8441: the child
 * resolves 30 optional dependencies whose full documents carry 120 MB of
 * install-irrelevant bulk under a 100 MB heap cap. Retaining the parsed
 * documents overshoots the cap by 100+ MB while the condensed working set
 * leaves ~3x headroom, so the guard is deterministic rather than
 * timing-sensitive.
 */
test('resolution completes within a small heap while registry documents carry megabytes of bulk', () => {
  const fixture = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures/resolve-bloated-metadata.mjs')
  // The child's stderr (the V8 heap-limit abort trace) is folded into the
  // error message because jest only prints the message, not properties.
  try {
    execFileSync(process.execPath, ['--max-old-space-size=100', fixture], {
      stdio: ['ignore', 'ignore', 'pipe'],
      timeout: 100_000,
    })
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'stderr' in err && err.stderr != null) {
      err.message += `\n\nChild stderr:\n${String(err.stderr)}`
    }
    throw err
  }
}, 120_000)
