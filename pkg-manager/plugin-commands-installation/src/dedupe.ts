import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { dedupeDiffCheck } from '@pnpm/dedupe.check'
import renderHelp from 'render-help'
import { type InstallCommandOptions } from './install'
import { installDeps } from './installDeps'
import { types as allTypes } from '@pnpm/config'
import pick from 'ramda/src/pick'

export function rcOptionsTypes () {
  return pick(['ignore-scripts'], allTypes)
}

export function cliOptionsTypes () {
  return {
    ...rcOptionsTypes(),
    check: Boolean,
  }
}

export const commandNames = ['dedupe']

export function help () {
  return renderHelp({
    description: 'Perform an install removing older dependencies in the lockfile if a newer version can be used.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          ...UNIVERSAL_OPTIONS,
          {
            description: 'Check if running dedupe would result in changes without installing packages or editing the lockfile. Exits with a non-zero status code if changes are possible.',
            name: '--check',
          },
          {
            description: "Don't run lifecycle scripts",
            name: '--ignore-scripts',
          },
        ],
      },
    ],
    url: docsUrl('dedupe'),
    usages: ['pnpm dedupe'],
  })
}

export interface DedupeCommandOptions extends InstallCommandOptions {
  readonly check?: boolean
}

export async function handler (opts: DedupeCommandOptions) {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  return installDeps({
    ...opts,
    dedupe: true,
    ignoreScripts: opts.ignoreScripts ?? false,
    include,
    includeDirect: include,
    lockfileCheck: opts.check ? dedupeDiffCheck : undefined,
  }, [])
}
