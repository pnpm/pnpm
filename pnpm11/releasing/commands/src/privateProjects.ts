import { toProjectDir, type WorkspaceProject } from '@pnpm/releasing.versioning'

/**
 * The workspace-relative dirs of the projects marked `"private": true`.
 *
 * A private project is never published, so a registry probe on its behalf is
 * futile and would read as "not published".
 */
export function privateProjectDirs (projects: WorkspaceProject[], workspaceDir: string): Set<string> {
  return new Set(projects.filter(isPrivate).map((project) => toProjectDir(workspaceDir, project.rootDir)))
}

/**
 * The manifest names that only private projects carry, for call sites that
 * hold a name rather than a dir — parked changelog sections and the ledger
 * both key on the manifest name. A name that a public project shares is left
 * out, so that project's release is still confirmed.
 */
export function privateOnlyProjectNames (projects: WorkspaceProject[]): Set<string> {
  const publicNames = new Set(projects.filter((project) => !isPrivate(project)).map(({ manifest }) => manifest.name))
  return new Set(projects
    .filter(isPrivate)
    .map(({ manifest }) => manifest.name)
    .filter((name): name is string => name != null && !publicNames.has(name)))
}

function isPrivate (project: WorkspaceProject): boolean {
  return project.manifest.private === true
}
