import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { createVersionsOverrider } from '@pnpm/hooks.read-package-hook'
import npa from '@pnpm/npm-package-arg'
import { parseBareSpecifier, workspacePrefToNpm } from '@pnpm/resolving.npm-resolver'
import type { BaseManifest, ProjectRootDir } from '@pnpm/types'
import { resolveWorkspaceRange } from '@pnpm/workspace.range-resolver'
import { map as mapValues } from 'ramda'

export interface BaseProject {
  manifest: BaseManifest
  rootDir: ProjectRootDir
}

export interface ProjectGraphNode<Pkg extends BaseProject> {
  package: Pkg
  dependencies: ProjectRootDir[]
}

/**
 * The `overrides` setting, applied to every manifest before its edges are
 * read, so the edges follow what the install resolves rather than what the
 * manifests declare: an override that points a dependency at a workspace
 * project (`workspace:`, `link:`, `file:`) makes that project a dependency,
 * whatever range the manifest declares and whether or not
 * `linkWorkspacePackages` is on.
 */
export interface ProjectsGraphOverrides {
  overrides: Record<string, string>
  catalogs?: Catalogs
  /** Anchors relative `link:` / `file:` override targets. */
  lockfileDir: string
}

export interface CreateProjectsGraphOptions {
  ignoreDevDeps?: boolean
  linkWorkspacePackages?: boolean
  overrides?: ProjectsGraphOverrides
}

export function createProjectsGraph<Pkg extends BaseProject> (projects: Pkg[], opts?: CreateProjectsGraphOptions): {
  graph: Record<ProjectRootDir, ProjectGraphNode<Pkg>>
  unmatched: Array<{ pkgName: string, range: string }>
} {
  const projectMap = createProjectMap(projects)
  const projectMapValues = Object.values(projectMap)
  let projectMapByManifestName: Record<string, BaseProject[] | undefined> | undefined
  let projectMapByDir: Record<string, BaseProject | undefined> | undefined
  const unmatched: Array<{ pkgName: string, range: string }> = []
  const applyOverrides = createOverridesApplier(opts?.overrides)
  const graph = mapValues((project) => ({
    dependencies: createNode(project),
    package: project,
  }), projectMap) as Record<ProjectRootDir, ProjectGraphNode<Pkg>>
  return { graph, unmatched }

  function createNode (project: BaseProject): string[] {
    const manifest = applyOverrides(project.manifest, project.rootDir)
    const dependencies = {
      ...manifest.peerDependencies,
      ...(!opts?.ignoreDevDeps && manifest.devDependencies),
      ...manifest.optionalDependencies,
      ...manifest.dependencies,
    }

    return Object.entries(dependencies)
      .map(([depName, rawSpec]) => {
        let spec!: { fetchSpec: string, type: string }
        const isWorkspaceSpec = rawSpec.startsWith('workspace:')
        try {
          if (isWorkspaceSpec) {
            const npmSpec = workspacePrefToNpm(rawSpec)
            if (isRelativePathSpec(npmSpec)) {
              // workspace:../foo / workspace:./foo aren't bare specifiers;
              // resolve them as directory dependencies below.
              rawSpec = npmSpec
            } else {
              let parsed: ReturnType<typeof parseBareSpecifier> = null
              try {
                parsed = parseBareSpecifier(npmSpec, depName, 'latest', '')
              } catch {
                // Defensive backstop for other malformed specs.
              }
              if (parsed) {
                rawSpec = parsed.fetchSpec
                depName = parsed.name
              } else {
                rawSpec = npmSpec
              }
            }
          }
          spec = npa.resolve(depName, rawSpec, project.rootDir)
        } catch {
          return ''
        }

        if (spec.type === 'directory') {
          projectMapByDir ??= getProjectMapByDir(projectMapValues)
          const resolvedPath = path.resolve(project.rootDir, spec.fetchSpec)
          const found = projectMapByDir[resolvedPath]
          if (found) {
            return found.rootDir
          }

          // Slow path; only needed when there are case mismatches on case-insensitive filesystems.
          const matchedProject = projectMapValues.find(p => path.relative(p.rootDir, spec.fetchSpec) === '')
          if (matchedProject == null) {
            return ''
          }
          projectMapByDir[resolvedPath] = matchedProject
          return matchedProject.rootDir
        }

        if (spec.type !== 'version' && spec.type !== 'range') return ''

        projectMapByManifestName ??= getProjectMapByManifestName(projectMapValues)
        const candidates = projectMapByManifestName[depName]
        if (!candidates || candidates.length === 0) return ''
        const versions = candidates.filter(({ manifest }) => manifest.version)
          .map(p => p.manifest.version) as string[]

        // explicitly check if false, backwards-compatibility (can be undefined)
        const strictWorkspaceMatching = opts?.linkWorkspacePackages === false && !isWorkspaceSpec
        if (strictWorkspaceMatching) {
          unmatched.push({ pkgName: depName, range: rawSpec })
          return ''
        }
        if (isWorkspaceSpec && versions.length === 0) {
          const matchedProject = candidates.find(p => p.manifest.name === depName)
          return matchedProject!.rootDir
        }
        if (versions.includes(rawSpec)) {
          const matchedProject = candidates.find(p => p.manifest.name === depName && p.manifest.version === rawSpec)
          return matchedProject!.rootDir
        }
        const matched = resolveWorkspaceRange(rawSpec, versions)
        if (!matched) {
          unmatched.push({ pkgName: depName, range: rawSpec })
          return ''
        }
        const matchedProject = candidates.find(p => p.manifest.name === depName && p.manifest.version === matched)
        return matchedProject!.rootDir
      })
      .filter(Boolean)
  }
}

function createOverridesApplier (overrides: ProjectsGraphOverrides | undefined): (manifest: BaseManifest, dir: string) => BaseManifest {
  if (overrides == null || Object.keys(overrides.overrides).length === 0) return (manifest) => manifest
  // The overrider clones the fields it rewrites and never awaits, so the
  // manifest it hands back is a plain value.
  return createVersionsOverrider(parseOverrides(overrides.overrides, overrides.catalogs), overrides.lockfileDir) as
    (manifest: BaseManifest, dir: string) => BaseManifest
}

function isRelativePathSpec (spec: string): boolean {
  return spec === '.' || spec === '..' || spec.startsWith('./') || spec.startsWith('../')
}

function createProjectMap (projects: BaseProject[]): Record<ProjectRootDir, BaseProject> {
  const projectMap: Record<ProjectRootDir, BaseProject> = {}
  for (const project of projects) {
    projectMap[project.rootDir] = project
  }
  return projectMap
}

function getProjectMapByManifestName (projectMapValues: BaseProject[]): Record<string, BaseProject[] | undefined> {
  const projectMapByManifestName: Record<string, BaseProject[] | undefined> = {}
  for (const project of projectMapValues) {
    if (project.manifest.name) {
      (projectMapByManifestName[project.manifest.name] ??= []).push(project)
    }
  }
  return projectMapByManifestName
}

function getProjectMapByDir (projectMapValues: BaseProject[]): Record<string, BaseProject | undefined> {
  const projectMapByDir: Record<string, BaseProject | undefined> = {}
  for (const project of projectMapValues) {
    projectMapByDir[path.resolve(project.rootDir)] = project
  }
  return projectMapByDir
}
