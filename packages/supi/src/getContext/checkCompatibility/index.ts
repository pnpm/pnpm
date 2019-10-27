import { LAYOUT_VERSION } from '@pnpm/constants'
import { Modules } from '@pnpm/modules-yaml'
import path = require('path')
import ModulesBreakingChangeError from './ModulesBreakingChangeError'
import UnexpectedStoreError from './UnexpectedStoreError'
import UnexpectedVirtualStoreDir from './UnexpectedVirtualStoreDirError'

export default function checkCompatibility (
  modules: Modules,
  opts: {
    storeDir: string,
    modulesDir: string,
    virtualStoreDir: string,
  },
) {
  // Important: comparing paths with path.relative()
  // is the only way to compare paths correctly on Windows
  // as of Node.js 4-9
  // See related issue: https://github.com/pnpm/pnpm/issues/996
  if (path.relative(modules.store, opts.storeDir) !== '') {
    throw new UnexpectedStoreError({
      actualStorePath: opts.storeDir,
      expectedStorePath: modules.store,
      modulesDir: opts.modulesDir,
    })
  }
  if (modules.virtualStoreDir && path.relative(modules.virtualStoreDir, opts.virtualStoreDir) !== '') {
    throw new UnexpectedVirtualStoreDir({
      actual: opts.virtualStoreDir,
      expected: modules.virtualStoreDir,
      modulesDir: opts.modulesDir,
    })
  }
  if (!modules.layoutVersion || modules.layoutVersion !== LAYOUT_VERSION) {
    throw new ModulesBreakingChangeError({
      modulesPath: opts.modulesDir,
    })
  }
}
