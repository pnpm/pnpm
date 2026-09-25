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
 * The dependency selectors a `--workspace` install resolves from the workspace
 * instead of the registry.
 *
 * `selectors` are the dependency names the update already matched, which is
 * empty both when the user named nothing and when the packages they named
 * matched no direct dependency. `userNamedDeps` separates those: only the
 * former expands to every workspace dependency in the manifest, and only the
 * latter treats a dependency missing from the workspace as an error.
 */
export function toWorkspaceSpecs (
  selectors: string[],
  opts: {
    manifest: ProjectManifest
    include: IncludedDependencies
    workspacePackages: WorkspacePackages
    userNamedDeps: boolean
    fromInteractiveUpdate?: boolean
  }
): string[] {
  if (selectors.length > 0) {
    return createWorkspaceSpecs(selectors, opts.workspacePackages, {
      skipPackagesOutsideWorkspace: !opts.userNamedDeps,
      preserveNonWorkspaceSpecs: opts.fromInteractiveUpdate,
    })
  }
  if (opts.userNamedDeps) return []
  return updateToWorkspacePackagesFromManifest(opts.manifest, opts.include, opts.workspacePackages)
}

/**
 * Rewrite dependency selectors to point at the workspace copies of the same
 * packages.
 *
 * A selector naming a package the workspace doesn't have is an error, since
 * `--workspace` was asked to link something that isn't there. Pass
 * `preserveNonWorkspaceSpecs` for interactive selection so external dependencies
 * keep their specifiers, or `skipPackagesOutsideWorkspace` when selectors were
 * derived from the manifest.
 */
export function createWorkspaceSpecs (
  specs: string[],
  workspacePackages: WorkspacePackages,
  opts?: {
    skipPackagesOutsideWorkspace?: boolean
    preserveNonWorkspaceSpecs?: boolean
  }
): string[] {
  const workspaceSpecs: string[] = []
  for (const spec of specs) {
    const parsed = parseWantedDependency(spec)
    if (!parsed.alias) throw new PnpmError('NO_PKG_NAME_IN_SPEC', `Cannot update/install from workspace through "${spec}"`)
    if (!workspacePackages.has(parsed.alias)) {
      if (opts?.preserveNonWorkspaceSpecs) {
        workspaceSpecs.push(spec)
        continue
      }
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
