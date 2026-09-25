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

  test('prompts without --yes flag', () => {
    expect(() => execPnpmSync(['install', '--config.optimistic-repeat-install=false'], execPnpmOpts)).toThrow('Aborted removal of modules directory due to no TTY')
  })

  test('skips prompt when --yes is passed', () => {
    expect(() => execPnpmSync(['install', '--yes', '--config.optimistic-repeat-install=false'], execPnpmOpts)).not.toThrow()
  })
})
