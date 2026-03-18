import path from 'node:path'

import npa from '@pnpm/npm-package-arg'
import { parseBareSpecifier, workspacePrefToNpm } from '@pnpm/resolving.npm-resolver'
import type { BaseManifest, ProjectRootDir } from '@pnpm/types'
import { resolveWorkspaceRange } from '@pnpm/workspace.range-resolver'
import { map as mapValues } from 'ramda'

export interface Package {
  manifest: BaseManifest
  rootDir: ProjectRootDir
}

export interface PackageNode<Pkg extends Package> {
  package: Pkg
  dependencies: ProjectRootDir[]
}

export function createProjectsGraph<Pkg extends Package> (projects: Pkg[], opts?: {
  ignoreDevDeps?: boolean
  linkWorkspacePackages?: boolean
}): {
    graph: Record<ProjectRootDir, PackageNode<Pkg>>
    unmatched: Array<{ pkgName: string, range: string }>
  } {
  const projectMap = createProjectMap(projects)
  const projectMapValues = Object.values(projectMap)
  let projectMapByManifestName: Record<string, Package[] | undefined> | undefined
  let projectMapByDir: Record<string, Package | undefined> | undefined
  const unmatched: Array<{ pkgName: string, range: string }> = []
  const graph = mapValues((project) => ({
    dependencies: createNode(project),
    package: project,
  }), projectMap) as Record<ProjectRootDir, PackageNode<Pkg>>
  return { graph, unmatched }

  function createNode (project: Package): string[] {
    const dependencies = {
      ...project.manifest.peerDependencies,
      ...(!opts?.ignoreDevDeps && project.manifest.devDependencies),
      ...project.manifest.optionalDependencies,
      ...project.manifest.dependencies,
    }

    return Object.entries(dependencies)
      .map(([depName, rawSpec]) => {
        let spec!: { fetchSpec: string, type: string }
        const isWorkspaceSpec = rawSpec.startsWith('workspace:')
        try {
          if (isWorkspaceSpec) {
            const { fetchSpec, name } = parseBareSpecifier(workspacePrefToNpm(rawSpec), depName, 'latest', '')!
            rawSpec = fetchSpec
            depName = name
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

function createProjectMap (projects: Package[]): Record<ProjectRootDir, Package> {
  const projectMap: Record<ProjectRootDir, Package> = {}
  for (const project of projects) {
    projectMap[project.rootDir] = project
  }
  return projectMap
}

function getProjectMapByManifestName (projectMapValues: Package[]): Record<string, Package[] | undefined> {
  const projectMapByManifestName: Record<string, Package[] | undefined> = {}
  for (const project of projectMapValues) {
    if (project.manifest.name) {
      (projectMapByManifestName[project.manifest.name] ??= []).push(project)
    }
  }
  return projectMapByManifestName
}

function getProjectMapByDir (projectMapValues: Package[]): Record<string, Package | undefined> {
  const projectMapByDir: Record<string, Package | undefined> = {}
  for (const project of projectMapValues) {
    projectMapByDir[path.resolve(project.rootDir)] = project
  }
  return projectMapByDir
}
