import type { ProjectManifest } from '@pnpm/types'

export type WorkspacePackageContainer =
  | Array<{ manifest: ProjectManifest, rootDir?: string }>
  | Map<string, unknown>
  | Record<string, unknown>

export type WorkspacePackageLookup =
  | WorkspacePackageContainer
  | ((depName: string) => ProjectManifest | undefined)

export function buildWorkspaceManifestGetter (
  workspacePackages?: WorkspacePackageLookup
): ((depName: string) => ProjectManifest | undefined) | undefined {
  if (!workspacePackages) return undefined
  if (typeof workspacePackages === 'function') {
    return workspacePackages
  }
  const byName = new Map<string, ProjectManifest>()
  collectWorkspaceManifests(workspacePackages, byName)
  return (depName: string) => byName.get(depName)
}

function collectWorkspaceManifests (
  workspacePackages: WorkspacePackageContainer,
  byName: Map<string, ProjectManifest>
): void {
  const add = (val: unknown) => addManifestEntry(val, byName)

  if (Array.isArray(workspacePackages)) {
    for (const pkg of workspacePackages) add(pkg)
    return
  }
  if (workspacePackages instanceof Map) {
    collectFromMap(workspacePackages, add)
    return
  }
  for (const val of Object.values(workspacePackages)) add(val)
}

function collectFromMap (map: Map<string, unknown>, add: (val: unknown) => void): void {
  for (const val of map.values()) {
    if (val instanceof Map) {
      for (const subVal of val.values()) add(subVal)
    } else {
      add(val)
    }
  }
}

function addManifestEntry (val: unknown, byName: Map<string, ProjectManifest>): void {
  const manifest = extractManifest(val)
  if (!manifest?.name) return
  const existing = byName.get(manifest.name)
  if (!existing || (!existing.version && manifest.version)) {
    byName.set(manifest.name, manifest)
  }
}

function extractManifest (val: unknown): ProjectManifest | undefined {
  if (!val || typeof val !== 'object') return undefined
  if ('manifest' in val && val.manifest && typeof val.manifest === 'object') {
    return val.manifest as ProjectManifest
  }
  if ('package' in val && val.package && typeof val.package === 'object' && 'manifest' in val.package && val.package.manifest && typeof val.package.manifest === 'object') {
    return val.package.manifest as ProjectManifest
  }
  if ('name' in val && typeof (val as Record<string, unknown>).name === 'string') {
    return val as ProjectManifest
  }
  return undefined
}
