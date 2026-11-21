import { publishedName } from '@pnpm/releasing.exportable-manifest'
import type { WorkspaceProject } from '@pnpm/releasing.versioning'

export { publishedName }

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
