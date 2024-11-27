import { docsUrl } from '@pnpm/cli-utils'
import { OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { dedupeDiffCheck } from '@pnpm/dedupe.check'
import { prepareExecutionEnv } from '@pnpm/plugin-commands-env'
import renderHelp from 'render-help'
import { type InstallCommandOptions, rcOptionsTypes as installCommandRcOptionsTypes } from './install'
import { installDeps } from './installDeps'
import omit from 'ramda/src/omit'

// In general, the "pnpm dedupe" command should use .npmrc options that "pnpm install" would also accept.
export function rcOptionsTypes (): Record<string, unknown> {
  // Some options on pnpm install (like --frozen-lockfile) don't make sense on pnpm dedupe.
  return omit(['frozen-lockfile'], installCommandRcOptionsTypes())
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    check: Boolean,
  }
}

export const commandNames = ['dedupe']

export function help (): string {
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
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          OPTIONS.storeDir,
          OPTIONS.virtualStoreDir,
          OPTIONS.globalDir,
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

export async function handler (opts: DedupeCommandOptions): Promise<void> {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  return installDeps({
    ...opts,
    dedupe: true,
    include,
    includeDirect: include,
    lockfileCheck: opts.check ? dedupeDiffCheck : undefined,
    prepareExecutionEnv: prepareExecutionEnv.bind(null, opts),
  }, [])
}
