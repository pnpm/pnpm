import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

// `pacquet` is fetched from the real npm registry — registry-mock doesn't
// carry it (or its platform-specific binary sub-packages). Pinned to a
// version known to ship the `configDependencies` integration surface this
// PR depends on; the test is gated on the public registry being reachable.
const PUBLIC_REGISTRY = '--config.registry=https://registry.npmjs.org/'
const PACQUET_VERSION = '0.2.2-9'

// Three back-to-back installs against the public registry can take a while
// on cold caches; raise the per-test timeout above jest's 5s default.
const TIMEOUT = 5 * 60 * 1000

test('pnpm install --frozen-lockfile delegates to pacquet when declared in configDependencies', async () => {
  prepare({
    dependencies: {
      'is-positive': '3.1.0',
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      pacquet: PACQUET_VERSION,
    },
  })

  // Step 1: populate the env lockfile + pnpm-lock.yaml, and materialize
  // pacquet (plus its platform-specific binary) under
  // `node_modules/.pnpm-config/pacquet`. This first install goes through
  // the JS path; pacquet only takes over on the frozen-install path
  // exercised in step 3.
  await execPnpm([PUBLIC_REGISTRY, 'install'])
  expect(fs.existsSync('node_modules/.pnpm-config/pacquet/bin/pacquet')).toBe(true)
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)

  // Step 2: wipe `node_modules` while leaving lockfiles intact. This is
  // the CI-style starting state — a checked-out repo with the lockfiles
  // committed and no installed modules.
  await fs.promises.rm('node_modules', { recursive: true, force: true })

  // Step 3: run `--frozen-lockfile`. Pnpm reinstalls `configDependencies`
  // first (pacquet lands again under .pnpm-config), then delegates the
  // dependency install to pacquet. The "Delegating install to pacquet"
  // info log proves the delegation branch ran rather than the JS
  // `headlessInstall` path.
  const { stderr, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
    {
      env: { pnpm_config_silent: 'false' },
      stdio: 'pipe',
      expectSuccess: true,
    }
  )
  expect(status).toBe(0)
  expect(stderr.toString()).toContain('Delegating install to pacquet')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)
