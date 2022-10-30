import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifestOnly,
} from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import {
  licensesDepsOfProjects,
} from '@pnpm/licenses'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { renderLicences } from './outputRenderer'
import { licensesRecursive } from './recursive'

export function rcOptionsTypes () {
  return {
    ...pick([
      'depth',
      'dev',
      'global-dir',
      'global',
      'json',
      'long',
      'optional',
      'production',
    ], allTypes),
    compatible: Boolean,
    table: Boolean,
  }
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
})

export const shorthands = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['licenses']

export function help () {
  return renderHelp({
    description: `Check for licenses packages. The check can be limited to a subset of the installed packages by providing arguments (patterns are supported).

Examples:
pnpm licenses
pnpm licenses --long
pnpm licenses gulp-* @babel/core`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print only versions that satisfy specs in package.json',
            name: '--compatible',
          },
          {
            description: 'By default, details about the outdated packages (such as a link to the repo) are not displayed. \
To display the details, pass this option.',
            name: '--long',
          },
          {
            description: 'Check for outdated dependencies in every package found in subdirectories \
or in every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Show information in JSON format',
            name: '--json',
          },
          {
            description: 'Prints the outdated packages in a list. Good for small consoles',
            name: '--no-table',
          },
          {
            description: 'Check only "dependencies" and "optionalDependencies"',
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Check only "devDependencies"',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Don\'t check "optionalDependencies"',
            name: '--no-optional',
          },
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('licenses'),
    usages: ['pnpm licenses [<pkg> ...]'],
  })
}

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export type LicensesCommandOptions = {
  compatible?: boolean
  long?: boolean
  recursive?: boolean
  table?: boolean
} & Pick<Config,
| 'allProjects'
| 'ca'
| 'cacheDir'
| 'cert'
| 'dev'
| 'dir'
| 'engineStrict'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'global'
| 'httpProxy'
| 'httpsProxy'
| 'key'
| 'localAddress'
| 'lockfileDir'
| 'networkConcurrency'
| 'noProxy'
| 'offline'
| 'optional'
| 'production'
| 'rawConfig'
| 'registries'
| 'selectedProjectsGraph'
| 'strictSsl'
| 'tag'
| 'userAgent'
| 'virtualStoreDir'
| 'modulesDir'
> & Partial<Pick<Config, 'userConfig'>>

export async function handler (
  opts: LicensesCommandOptions,
  params: string[] = []
) {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  if (opts.recursive && (opts.selectedProjectsGraph != null)) {
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    return licensesRecursive(pkgs, params, { ...opts, include })
  }
  const manifest = await readProjectManifestOnly(opts.dir, opts)
  const packages = [
    {
      dir: opts.dir,
      manifest,
    },
  ]
  const [licensePackages] = await licensesDepsOfProjects(packages, params, {
    ...opts,
    fullMetadata: opts.long,
    ignoreDependencies: new Set(manifest?.pnpm?.updateConfig?.ignoreDependencies ?? []),
    include,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })

  if (licensePackages.length === 0) return { output: '', exitCode: 0 }

  return renderLicences(licensePackages, opts)
}
