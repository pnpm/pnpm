import { LAYOUT_VERSION } from '@pnpm/constants'
import { Modules } from '@pnpm/modules-yaml'
import path = require('path')
import ModulesBreakingChangeError from './ModulesBreakingChangeError'
import UnexpectedStoreError from './UnexpectedStoreError'

export default function checkCompatibility (
  modules: Modules,
  opts: {
    storePath: string,
    modulesPath: string,
  },
) {
  // Important: comparing paths with path.relative()
  // is the only way to compare paths correctly on Windows
  // as of Node.js 4-9
  // See related issue: https://github.com/pnpm/pnpm/issues/996
  if (path.relative(modules.store, opts.storePath) !== '') {
    throw new UnexpectedStoreError({
      actualStorePath: opts.storePath,
      expectedStorePath: modules.store,
    })
  }
  if (!modules.layoutVersion || modules.layoutVersion !== LAYOUT_VERSION) {
    throw new ModulesBreakingChangeError({
      modulesPath: opts.modulesPath,
    })
  }
}
