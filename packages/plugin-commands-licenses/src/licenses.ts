import { docsUrl } from '@pnpm/cli-utils'
import { Config, types as allTypes, UniversalOptions } from '@pnpm/config'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { Registries } from '@pnpm/types'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import licenseCheck from './licenseChecker'
import { LicenseComplianceReport } from './types'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {
    ...pick([
      'dev',
      'json',
      'only',
      'optional',
      'production',
      'registry',
    ], allTypes),
  }
}

export const shorthands = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['licenses']

export function help () {
  return renderHelp({
    description: 'Checks for license compliance of the installed packages packages.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Output license compliant report in JSON format',
            name: '--json',
          },
          {
            description: 'Only check "devDependencies"',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Only check "dependencies" and "optionalDependencies"',
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Don\'t check "optionalDependencies"',
            name: '--no-optional',
          },
        ],
      },
    ],
    url: docsUrl('licenses'),
    usages: ['pnpm licenses [options]'],
  })
}

export async function handler (
  opts: Pick<UniversalOptions, 'dir'> & {
    json?: boolean
    lockfileDir?: string
    registries: Registries
  } & Pick<Config, 'ca'
  | 'production'
  | 'dev'
  | 'optional'
  | 'virtualStoreDir'
  >
) {
  const lockfile = await readWantedLockfile(opts.lockfileDir ?? opts.dir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError('LICENSES_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot license compliance check a project without a lockfile`)
  }

  if (!opts.virtualStoreDir) {
    throw new PnpmError('LICENSES_NO_VIRTUAL_STORE_DIRECTORY', 'No virtual store directory found: Cannot license compliance check a project without a virtual store directory')
  }

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  let complianceReport!: LicenseComplianceReport
  try {
    complianceReport = await licenseCheck(lockfile, {
      virtualStoreDir: opts.virtualStoreDir,
      dir: opts.dir,
      include,
    })
  } catch (err: unknown) {
    return {
      exitCode: 0,
      output: (err as Error).message,
    }
  }
  console.log('complianceReport:', complianceReport)

  if (opts.json) {
    return {
      exitCode: 1,
      output: JSON.stringify({ test: 'test' }, null, 2),
    }
  }

  const output = 'test-output-data'
  // const auditLevel = AUDIT_LEVEL_NUMBER[opts.auditLevel ?? 'low']
  // const advisories = Object.values(auditReport.advisories)
  //   .filter(({ severity }) => AUDIT_LEVEL_NUMBER[severity] >= auditLevel)
  //   .sort((a1, a2) => AUDIT_LEVEL_NUMBER[a2.severity] - AUDIT_LEVEL_NUMBER[a1.severity])
  // for (const advisory of advisories) {
  //   output += table([
  //     [AUDIT_COLOR[advisory.severity](advisory.severity), chalk.bold(advisory.title)],
  //     ['Package', advisory.module_name],
  //     ['Vulnerable versions', advisory.vulnerable_versions],
  //     ['Patched versions', advisory.patched_versions],
  //     ['More info', advisory.url],
  //   ], TABLE_OPTIONS)
  // }
  return {
    exitCode: output ? 1 : 0,
    // output: `${output}${reportSummary(auditReport.metadata.vulnerabilities, totalVulnerabilityCount)}`,
    output: `${output}`,
  }
}
