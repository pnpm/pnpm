import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { whyForPackages } from '@pnpm/list'
import { pick } from 'ramda'
import renderHelp from 'render-help'
import { computeInclude, resolveFinders, determineReportAs, SHARED_CLI_HELP_OPTIONS, BASE_RC_OPTION_KEYS } from './common.js'
import { type ListCommandOptions, EXCLUDE_PEERS_HELP } from './list.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([...BASE_RC_OPTION_KEYS, 'depth'], allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  'exclude-peers': Boolean,
  recursive: Boolean,
  'find-by': [String, Array],
})

export { shorthands } from './common.js'

export const commandNames = ['why']

export function help (): string {
  return renderHelp({
    description: `Shows the packages that depend on <pkg>
For example: pnpm why babel-* eslint-*`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          ...SHARED_CLI_HELP_OPTIONS,
          {
            description: 'Max display depth of the reverse dependency tree',
            name: '--depth <number>',
          },
          EXCLUDE_PEERS_HELP,
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('why'),
    usages: [
      'pnpm why <pkg> ...',
    ],
  })
}

export async function handler (
  opts: ListCommandOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0 && opts.findBy == null) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm why` requires the package name or --find-by=<finder-name>')
  }

  const include = computeInclude(opts)
  const finders = resolveFinders(opts)
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const reportAs = determineReportAs(opts)
  const depth = opts.cliOptions?.['depth'] as number | undefined

  const projectPaths = opts.recursive && opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph)
    : [opts.dir]

  return whyForPackages(params, projectPaths, {
    depth,
    include,
    long: opts.long,
    lockfileDir,
    reportAs,
    modulesDir: opts.modulesDir,
    checkWantedLockfileOnly: opts.lockfileOnly,
    finders,
  })
}
