import {oneLine, stripIndent} from 'common-tags'
import path = require('path')
import semver = require('semver')
import {PnpmError, PnpmErrorCode} from '../../errorTypes'
import {LAYOUT_VERSION, Modules} from '../../fs/modulesController'
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
      additionalInformation: 'The change was needed to make `independent-leafs` not the default installation layout',
      modulesPath: opts.modulesPath,
      relatedIssue: 821,
    })
  }
}
