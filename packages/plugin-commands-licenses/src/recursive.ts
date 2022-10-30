import {
  licensesDepsOfProjects,
  LicensePackage,
} from '@pnpm/licenses'
import {
  DependenciesField,
  IncludedDependencies,
  ProjectManifest,
} from '@pnpm/types'

import isEmpty from 'ramda/src/isEmpty'
import { renderLicencesInWorkspace } from './outputRenderer'
import {
  LicensesCommandOptions,
} from './licenses'

export interface LicensesInWorkspace extends LicensePackage {
  belongsTo: DependenciesField
  current?: string
  dependentPkgs: Array<{ location: string, manifest: ProjectManifest }>
  latest?: string
  packageName: string
}

export async function licensesRecursive (
  pkgs: Array<{ dir: string, manifest: ProjectManifest }>,
  params: string[],
  opts: LicensesCommandOptions & { include: IncludedDependencies }
) {
  const licensesMap = {} as Record<string, LicensesInWorkspace>
  const rootManifest = pkgs.find(({ dir }) => dir === opts.lockfileDir ?? opts.dir)
  const LicensePackagesByProject = await licensesDepsOfProjects(pkgs, params, {
    ...opts,
    fullMetadata: opts.long,
    ignoreDependencies: new Set(rootManifest?.manifest?.pnpm?.updateConfig?.ignoreDependencies ?? []),
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })
  for (let i = 0; i < LicensePackagesByProject.length; i++) {
    const { dir, manifest } = pkgs[i]
    LicensePackagesByProject[i].forEach((licensePkg: LicensePackage) => {
      const key = JSON.stringify([licensePkg.packageName, licensePkg.version, licensePkg.belongsTo])
      if (!licensesMap[key]) {
        licensesMap[key] = { ...licensePkg, dependentPkgs: [] }
      }
      licensesMap[key].dependentPkgs.push({ location: dir, manifest })
    })
  }

  if (isEmpty(licensesMap)) return { output: '', exitCode: 0 }

  return renderLicencesInWorkspace(licensesMap, opts)
}
