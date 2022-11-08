import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifestOnly,
} from '@pnpm/cli-utils'
import { getStorePath } from '@pnpm/store-path'
import { CompletionFunc } from '@pnpm/command'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import {
  FILTERING,
  OPTIONS,
  UNIVERSAL_OPTIONS,
} from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { findDependencyLicenses } from '@pnpm/license-scanner'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { renderLicences } from './outputRenderer'

export function rcOptionsTypes () {
  return {
    ...pick(
      ['dev', 'global-dir', 'global', 'json', 'long', 'optional', 'production'],
      allTypes
    ),
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
            description:
              'By default, details about the packages (such as a link to the repo) are not displayed. \
To display the details, pass this option.',
            name: '--long',
          },
          {
            description: 'Show information in JSON format',
            name: '--json',
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
    usages: ['pnpm licenses [options]'],
  })
}

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export type LicensesCommandOptions = {
  compatible?: boolean
  long?: boolean
  recursive?: boolean
  json?: boolean
} & Pick<
Config,
| 'dev'
| 'dir'
| 'lockfileDir'
| 'registries'
| 'optional'
| 'production'
| 'storeDir'
| 'virtualStoreDir'
| 'modulesDir'
| 'pnpmHomeDir'
> &
Partial<Pick<Config, 'userConfig'>>

export async function handler (
  opts: LicensesCommandOptions,
  params: string[] = []
) {
  const lockfile = await readWantedLockfile(opts.lockfileDir ?? opts.dir, {
    ignoreIncompatible: true,
  })
  if (lockfile == null) {
    throw new PnpmError(
      'LICENSES_NO_LOCKFILE',
      `No ${WANTED_LOCKFILE} found: Cannot check a project without a lockfile`
    )
  }

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  const manifest = await readProjectManifestOnly(opts.dir, {})

  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })

  const licensePackages = await findDependencyLicenses({
    include,
    lockfileDir: opts.dir,
    storeDir,
    virtualStoreDir: opts.virtualStoreDir ?? '.',
    modulesDir: opts.modulesDir,
    registries: opts.registries,
    wantedLockfile: lockfile,
    manifest,
  })

  if (licensePackages.length === 0)
    return { output: 'No licenses in packages found', exitCode: 0 }

  return renderLicences(licensePackages, opts)
}
