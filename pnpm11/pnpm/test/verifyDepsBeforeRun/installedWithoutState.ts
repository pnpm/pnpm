import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm, execPnpmSync } from '../utils/index.js'

const HELLO = 'node -e "process.stdout.write(\'hello-from-script\')"'

test('pnpm run executes when the workspace state file is missing and node_modules matches the lockfile', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
    },
    scripts: {
      hello: HELLO,
    },
  })

  await execPnpm(['install'])
  fs.rmSync('node_modules/.pnpm-workspace-state-v1.json')
  const storeBlocker = path.resolve('not-a-store')
  fs.writeFileSync(storeBlocker, 'x')

  const { stdout, stderr } = execPnpmSync(['run', 'hello'], {
    env: unreachableStoreEnv(storeBlocker),
    expectSuccess: true,
  })

  expect(stdout.toString()).toContain('hello-from-script')
  expect(stderr.toString()).not.toContain('ERR_SQLITE')
  expect(stderr.toString()).not.toContain('Verifying lockfile')
})

test('pnpm run still tries to install when the manifest no longer matches the lockfile', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
    },
    scripts: {
      hello: HELLO,
    },
  })

  await execPnpm(['install'])
  fs.writeFileSync('node_modules/.pnpm-workspace-state-v1.json', '{not-json')
  project.writePackageJson({
    dependencies: {
      '@pnpm.e2e/foo': '100.1.0',
    },
    scripts: {
      hello: HELLO,
    },
  })
  const storeBlocker = path.resolve('not-a-store')
  fs.writeFileSync(storeBlocker, 'x')

  const { status, stdout } = execPnpmSync(['run', 'hello'], {
    env: unreachableStoreEnv(storeBlocker),
  })

  expect(status).not.toBe(0)
  expect(stdout.toString()).not.toContain('hello-from-script')
})

function unreachableStoreEnv (storeDir: string): Record<string, string> {
  return {
    pnpm_config_fetch_retries: '0',
    pnpm_config_fetch_retry_maxtimeout: '0',
    pnpm_config_fetch_retry_mintimeout: '0',
    pnpm_config_registry: 'http://127.0.0.1:1/',
    pnpm_config_store_dir: storeDir,
  }
}
