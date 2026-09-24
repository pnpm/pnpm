import { toProjectDir, type WorkspaceProject } from '@pnpm/releasing.versioning'

/**
 * The workspace-relative dirs of the projects marked `"private": true`.
 *
 * A private project is never published, so every registry probe a release
 * makes on its behalf is both futile and a failure the release would have to
 * read as "not published". {@link resolveUnpublishedDirs} and the
 * publication check behind `verifyPublished` skip these projects instead.
 */
export function privateProjectDirs (projects: WorkspaceProject[], workspaceDir: string): Set<string> {
  return new Set(projects.filter(isPrivate).map((project) => toProjectDir(workspaceDir, project.rootDir)))
}

/**
 * The manifest names of the projects marked `"private": true`, for call sites
 * that hold a name rather than a dir — parked changelog sections and the
 * ledger both key on the manifest name.
 */
export function privateProjectNames (projects: WorkspaceProject[]): Set<string> {
  return new Set(projects.filter(isPrivate).map(({ manifest }) => manifest.name).filter((name): name is string => name != null))
}

function isPrivate (project: WorkspaceProject): boolean {
  return project.manifest.private === true
}
