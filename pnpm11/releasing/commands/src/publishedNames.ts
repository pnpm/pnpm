import type { WorkspaceProject } from '@pnpm/releasing.versioning'
import type { BaseManifest } from '@pnpm/types'

/**
 * The name a manifest publishes under — its `publishConfig.name` rename, or
 * the name it carries in the workspace.
 *
 * The workspace — and so every parked changelog section, the ledger, every
 * intent, and every dependent — keys on the manifest name, while the registry
 * only ever sees the published one. Anything that addresses a project *at the
 * registry* has to resolve the name through here first.
 */
export function publishedName (manifest: Pick<BaseManifest, 'name' | 'publishConfig'>): string | undefined {
  const renamed = manifest.publishConfig?.name
  return typeof renamed === 'string' && renamed !== '' ? renamed : manifest.name
}

/**
 * Manifest name → published name, for every project that renames itself.
 * Projects that publish under their manifest name are absent, so a lookup miss
 * means "no rename". For call sites that hold a name rather than a manifest.
 */
export function publishedNameByManifestName (projects: WorkspaceProject[]): Map<string, string> {
  const renames = new Map<string, string>()
  for (const { manifest } of projects) {
    const published = publishedName(manifest)
    if (manifest.name && published != null && published !== manifest.name) {
      renames.set(manifest.name, published)
    }
  }
  return renames
}
