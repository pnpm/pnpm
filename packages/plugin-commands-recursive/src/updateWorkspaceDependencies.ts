import PnpmError from '@pnpm/error'
import { WorkspacePackages } from '@pnpm/resolver-base'
import { ImporterManifest, IncludedDependencies } from '@pnpm/types'
import { parseWantedDependency } from '@pnpm/utils'

export function updateToWorkspacePackagesFromManifest (manifest: ImporterManifest, include: IncludedDependencies, workspacePackages: WorkspacePackages) {
  const allDeps = {
    ...(include.devDependencies ? manifest.devDependencies : {}),
    ...(include.dependencies ? manifest.dependencies : {}),
    ...(include.optionalDependencies ? manifest.optionalDependencies : {}),
  } as { [name: string]: string }
  const updateSpecs = []
  for (const depName of Object.keys(allDeps)) {
    if (workspacePackages[depName]) {
      updateSpecs.push(`${depName}@workspace:*`)
    }
  }
  return updateSpecs
}

export function createWorkspaceSpecs (specs: string[], workspacePackages: WorkspacePackages) {
  return specs.map((spec) => {
    const parsed = parseWantedDependency(spec)
    if (!parsed.alias) throw new PnpmError('NO_PKG_NAME_IN_SPEC', `Cannot update/install from workspace through "${spec}"`)
    if (!workspacePackages[parsed.alias]) throw new PnpmError('WORKSPACE_PACKAGE_NOT_FOUND', `"${parsed.alias}" not found in the workspace`)
    if (!parsed.pref) return `${parsed.alias}@workspace:*`
    if (parsed.pref.startsWith('workspace:')) return spec
    return `${parsed.alias}@workspace:${parsed.pref}`
  })
}
