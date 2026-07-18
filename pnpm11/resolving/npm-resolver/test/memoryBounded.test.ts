import { execFileSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@jest/globals'

/**
 * Regression guard for https://github.com/pnpm/pnpm/issues/8441: resolving
 * must not retain full-document bulk, so it completes inside a small heap.
 *
 * The child process resolves 30 optional dependencies (the plain-install
 * route that fetches full metadata) whose documents each carry 4 MB of
 * install-irrelevant bulk — 120 MB total. Retaining the parsed documents,
 * as the resolver did before condensing, overflows the 100 MB cap after a
 * dozen packages; the condensed working set is a few KB per package, which
 * leaves the child roughly a 3x headroom over its ~30 MB baseline. That
 * margin is what keeps this deterministic rather than timing-sensitive: a
 * retention regression overshoots the cap by 100+ MB, not by noise.
 */
test('resolution completes within a small heap while registry documents carry megabytes of bulk', () => {
  const fixture = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures/resolve-bloated-metadata.mjs')
  // execFileSync throws on a non-zero exit — an OOM-killed child (V8 aborts
  // with "Ineffective mark-compacts near heap limit") fails the test, with
  // the child's stderr attached for diagnosis.
  expect(() => {
    execFileSync(process.execPath, ['--max-old-space-size=100', fixture], {
      stdio: ['ignore', 'ignore', 'pipe'],
      timeout: 100_000,
    })
  }).not.toThrow()
}, 120_000)
