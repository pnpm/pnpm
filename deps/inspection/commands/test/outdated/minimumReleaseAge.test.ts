/// <reference path="../../../../../__typings__/index.d.ts" />
import fs from 'node:fs'
import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { outdated } from '@pnpm/deps.inspection.commands'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'

const f = fixtures(import.meta.dirname)
const hasOutdatedDepsFixture = f.find('has-outdated-deps')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const OUTDATED_OPTIONS = {
  cacheDir: 'cache',
  fetchRetries: 1,
  fetchRetryFactor: 1,
  fetchRetryMaxtimeout: 60,
  fetchRetryMintimeout: 10,
  global: false,
  networkConcurrency: 16,
  offline: false,
  configByUri: {},
  registries: { default: REGISTRY_URL },
  strictSsl: false,
  tag: 'latest',
  userAgent: '',
  userConfig: {},
}

function loadHasOutdatedDeps (): void {
  tempDir()
  fs.mkdirSync(path.resolve('node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))
}

// A cutoff so far in the past that EVERY published version is "too new" to be
// mature — the same technique as the install-side minimumReleaseAge suite
// (allImmatureMinimumReleaseAge). Date-independent, so it does not depend on
// any registry-mock package's historical publish timestamps.
const allImmatureMinimumReleaseAge = Date.now() / (60 * 1000)

test('pnpm outdated baseline (no minimumReleaseAge): newer versions are offered', async () => {
  loadHasOutdatedDeps()

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  // Sanity: without an age policy, outdated offers the newest registry versions.
  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toContain('is-negative')
  expect(stripAnsi(output)).toContain('2.1.0')
})

// Repro for: `pnpm outdated` does not apply a configured `minimumReleaseAge`.
//
// Observed in the field on pnpm 11.1.2 with `minimumReleaseAge: 10080` (7d) in
// pnpm-workspace.yaml: `pnpm -r outdated` still surfaced a dependency upgrade
// whose target version was published ~3 days earlier, well inside the window.
// The install/resolution path honors the same setting, so the gap is specific
// to the `outdated` command's age filtering.
//
// With every version immature, a working age filter means there is no mature
// version newer than what is installed, so `outdated` must report nothing for
// is-negative (getManifest returns null on NO_(MATURE_)MATCHING_VERSION). If
// the bug is present, the configured age policy is ignored and `outdated`
// still offers is-negative@2.1.0 — this test goes red.
test('pnpm outdated honors minimumReleaseAge: immature newer versions are not offered', async () => {
  loadHasOutdatedDeps()

  const { output } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    minimumReleaseAge: allImmatureMinimumReleaseAge,
  })

  // 2.1.0 is far newer than the (epoch) cutoff, so a correct age filter must
  // not present it as an available upgrade.
  expect(stripAnsi(output)).not.toContain('2.1.0')
})
