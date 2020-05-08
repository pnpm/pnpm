import PnpmError from '@pnpm/error'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import { WorkspacePackages } from '@pnpm/resolver-base'
import { IncludedDependencies, ProjectManifest } from '@pnpm/types'

export function updateToWorkspacePackagesFromManifest (manifest: ProjectManifest, include: IncludedDependencies, workspacePackages: WorkspacePackages) {
  const allDeps = {
    ...(include.devDependencies ? manifest.devDependencies : {}),
    ...(include.dependencies ? manifest.dependencies : {}),
    ...(include.optionalDependencies ? manifest.optionalDependencies : {}),
  } as Record<string, string>
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
