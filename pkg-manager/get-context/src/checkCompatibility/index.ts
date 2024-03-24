import path from 'node:path'

import type { Modules } from '@pnpm/types'
import { LAYOUT_VERSION } from '@pnpm/constants'

import { UnexpectedStoreError } from './UnexpectedStoreError.js'
import { ModulesBreakingChangeError } from './ModulesBreakingChangeError.js'
import { UnexpectedVirtualStoreDirError } from './UnexpectedVirtualStoreDirError.js'

export function checkCompatibility(
  modules: Modules,
  opts: {
    storeDir: string
    modulesDir: string
    virtualStoreDir: string
  }
): void {
  if (!modules.layoutVersion || modules.layoutVersion !== LAYOUT_VERSION) {
    throw new ModulesBreakingChangeError({
      modulesPath: opts.modulesDir,
    })
  }

  // Important: comparing paths with path.relative()
  // is the only way to compare paths correctly on Windows
  // as of Node.js 4-9
  // See related issue: https://github.com/pnpm/pnpm/issues/996
  if (
    !modules.storeDir ||
    path.relative(modules.storeDir, opts.storeDir) !== ''
  ) {
    throw new UnexpectedStoreError({
      actualStorePath: opts.storeDir,
      expectedStorePath: modules.storeDir,
      modulesDir: opts.modulesDir,
    })
  }

  if (
    modules.virtualStoreDir &&
    path.relative(modules.virtualStoreDir, opts.virtualStoreDir) !== ''
  ) {
    throw new UnexpectedVirtualStoreDirError({
      actual: opts.virtualStoreDir,
      expected: modules.virtualStoreDir,
      modulesDir: opts.modulesDir,
    })
  }
}
