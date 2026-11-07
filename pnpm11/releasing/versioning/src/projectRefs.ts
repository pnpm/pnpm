import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import type { ProjectManifest } from '@pnpm/types'

import { normalizeProjectDir } from './ledger.js'

export interface WorkspaceProject {
  rootDir: string
  manifest: ProjectManifest
}

/**
 * Whether a package reference is a workspace-relative directory path rather
 * than a package name — the additive extension to the changesets format,
 * needed only when workspace projects share a published name.
 */
export function isDirRef (ref: string): boolean {
  return ref.startsWith('./')
}

/**
 * Resolves package references — bare names, or `./`-prefixed
 * workspace-relative directories — against the workspace. Names are aliases:
 * one that matches several projects cannot identify any of them and callers
 * must treat it as an error, never a silent pick.
 */
export interface ProjectRefIndex {
  /** The directories a reference resolves to: `[]` unknown, 2+ ambiguous. */
  refToDirs: (ref: string) => string[]
  nameToDirs: (name: string) => string[]
}

export function indexProjectRefs (
  projects: ReadonlyArray<{ rootDir: string, manifest: { name?: string } }>,
  workspaceDir: string
): ProjectRefIndex {
  const dirs = new Set<string>()
  const dirsByName = new Map<string, string[]>()
  for (const project of projects) {
    const dir = toProjectDir(workspaceDir, project.rootDir)
    dirs.add(dir)
    const name = project.manifest.name
    if (name == null) continue
    let named = dirsByName.get(name)
    if (named == null) {
      named = []
      dirsByName.set(name, named)
    }
    named.push(dir)
  }
  return {
    refToDirs: (ref) => {
      if (isDirRef(ref)) {
        const dir = normalizeProjectDir(ref)
        return dirs.has(dir) ? [dir] : []
      }
      return dirsByName.get(ref) ?? []
    },
    nameToDirs: (name) => dirsByName.get(name) ?? [],
  }
}

/** The workspace-relative directory of a project, in canonical spelling. */
export function toProjectDir (workspaceDir: string, rootDir: string): string {
  return normalizeProjectDir(path.relative(workspaceDir, rootDir))
}

/**
 * The workspace-relative dirs of the projects marked `"private": true`.
 *
 * A private project is never published, so a registry probe on its behalf is
 * futile and would read as "not published", and no tarball can ever carry its
 * changelog.
 */
export function privateProjectDirs (projects: WorkspaceProject[], workspaceDir: string): Set<string> {
  return new Set(projects
    .filter(({ manifest }) => manifest.private === true)
    .map(({ rootDir }) => toProjectDir(workspaceDir, rootDir)))
}

/**
 * Resolves a package reference from `versioning` configuration. An unknown
 * reference is skipped — configuration may outlive a removed project — but an
 * ambiguous name is an error: it cannot be attributed, and silence here is
 * exactly the name-keying flaw this engine exists to fix.
 */
export function resolveConfigRef (refs: ProjectRefIndex, ref: string, settingName: string): string[] {
  const dirs = refs.refToDirs(ref)
  if (dirs.length > 1) {
    throw new PnpmError(
      'VERSIONING_AMBIGUOUS_PACKAGE',
      `${settingName} references ${ref}, which matches multiple workspace projects: ${dirs.map((dir) => `./${dir}`).join(', ')}. Reference the project by directory instead.`
    )
  }
  return dirs
}
