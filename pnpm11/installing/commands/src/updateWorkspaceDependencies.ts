import { PnpmError } from '@pnpm/error'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { IncludedDependencies, ProjectManifest } from '@pnpm/types'

export function updateToWorkspacePackagesFromManifest (
  manifest: ProjectManifest,
  include: IncludedDependencies,
  workspacePackages: WorkspacePackages
): string[] {
  const allDeps = {
    ...(include.devDependencies ? manifest.devDependencies : {}),
    ...(include.dependencies ? manifest.dependencies : {}),
    ...(include.optionalDependencies ? manifest.optionalDependencies : {}),
  } as Record<string, string>
  return Object.keys(allDeps)
    .filter(depName => workspacePackages.has(depName))
    .map(depName => `${depName}@workspace:*`)
}

/**
 * Rewrite dependency selectors to point at the workspace copies of the same
 * packages.
 *
 * A selector naming a package the workspace doesn't have is an error, since
 * `--workspace` was asked to link something that isn't there. Pass
 * `skipPackagesOutsideWorkspace` when the selectors weren't named by the user
 * (they were derived from the manifest), where a registry dependency is
 * expected and simply keeps its specifier.
 */
export function createWorkspaceSpecs (
  specs: string[],
  workspacePackages: WorkspacePackages,
  opts?: { skipPackagesOutsideWorkspace?: boolean }
): string[] {
  const workspaceSpecs: string[] = []
  for (const spec of specs) {
    const parsed = parseWantedDependency(spec)
    if (!parsed.alias) throw new PnpmError('NO_PKG_NAME_IN_SPEC', `Cannot update/install from workspace through "${spec}"`)
    if (!workspacePackages.has(parsed.alias)) {
      if (opts?.skipPackagesOutsideWorkspace) continue
      throw new PnpmError('WORKSPACE_PACKAGE_NOT_FOUND', `"${parsed.alias}" not found in the workspace`)
    }
    if (!parsed.bareSpecifier) {
      workspaceSpecs.push(`${parsed.alias}@workspace:*`)
    } else if (parsed.bareSpecifier.startsWith('workspace:')) {
      workspaceSpecs.push(spec)
    } else {
      workspaceSpecs.push(`${parsed.alias}@workspace:${parsed.bareSpecifier}`)
    }
  }
  return workspaceSpecs
}
