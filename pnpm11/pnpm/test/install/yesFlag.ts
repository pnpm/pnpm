import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import type { PackageManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'

import type { ExecPnpmSyncOpts } from '../utils/execPnpm.js'
import { execPnpmSync } from '../utils/index.js'

const basicPackageManifest = loadJsonFileSync<PackageManifest>(path.join(import.meta.dirname, '../utils/simple-package.json'))

describe('pnpm install --yes', () => {
  beforeEach(() => {
    prepare(basicPackageManifest)
    execPnpmSync(['install'])

    // Write an incompatible layoutVersion to force a module purge prompt.
    fs.writeFileSync('node_modules/.modules.yaml', 'layoutVersion: 1')
  })

  const execPnpmOpts: ExecPnpmSyncOpts = {
    expectSuccess: true,
    env: { CI: 'false' },
  }

  test('auto-proceeds without --yes flag in non-interactive environment', () => {
    const result = execPnpmSync(['install', '--config.optimistic-repeat-install=false'], execPnpmOpts)
    expect(result.stdout.toString()).toContain('Non-interactive terminal detected. Automatically proceeding with modules directory purge')
  })

  test('skips prompt when --yes is passed', () => {
    const result = execPnpmSync(['install', '--yes', '--config.optimistic-repeat-install=false'], execPnpmOpts)
    expect(result.stdout.toString()).not.toContain('Non-interactive terminal detected')
  })
})
