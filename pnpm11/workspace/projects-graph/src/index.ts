import path from 'node:path'

import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
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

export interface CreateProjectsGraphOptions {
  catalogs?: Catalogs
  ignoreDevDeps?: boolean
  linkWorkspacePackages?: boolean
}

export function createProjectsGraph<Pkg extends BaseProject> (projects: Pkg[], opts?: CreateProjectsGraphOptions): {
  graph: Record<ProjectRootDir, ProjectGraphNode<Pkg>>
  unmatched: Array<{ pkgName: string, range: string }>
} {
  const projectMap = createProjectMap(projects)
  const projectMapValues = Object.values(projectMap)
  const unmatched: Array<{ pkgName: string, range: string }> = []
  const context: GraphResolutionContext = {
    catalogs: opts?.catalogs ?? {},
    ignoreDevDeps: opts?.ignoreDevDeps,
    linkWorkspacePackages: opts?.linkWorkspacePackages,
    projectMapValues,
    unmatched,
  }

  const graph = mapValues((project) => ({
    dependencies: resolveProjectDependencies(project, context),
    package: project,
  }), projectMap) as Record<ProjectRootDir, ProjectGraphNode<Pkg>>

  return { graph, unmatched }
}

interface GraphResolutionContext {
  catalogs: Catalogs
  ignoreDevDeps?: boolean
  linkWorkspacePackages?: boolean
  projectMapValues: BaseProject[]
  projectMapByManifestName?: Map<string, BaseProject[]>
  projectMapByDir?: Record<string, BaseProject | undefined>
  unmatched: Array<{ pkgName: string, range: string }>
}

function resolveProjectDependencies (project: BaseProject, ctx: GraphResolutionContext): string[] {
  const dependencies = {
    ...project.manifest.peerDependencies,
    ...(!ctx.ignoreDevDeps && project.manifest.devDependencies),
    ...project.manifest.optionalDependencies,
    ...project.manifest.dependencies,
  }

  return Object.entries(dependencies)
    .map(([depName, rawSpec]) => resolveDepToProject(project, depName, rawSpec, ctx))
    .filter((rootDir): rootDir is string => Boolean(rootDir))
}

function resolveDepToProject (
  project: BaseProject,
  depName: string,
  rawSpec: string,
  ctx: GraphResolutionContext
): string {
  const catalogResolution = resolveFromCatalog(ctx.catalogs, { alias: depName, bareSpecifier: rawSpec })
  if (catalogResolution.type === 'found') {
    rawSpec = catalogResolution.resolution.specifier
  }
  const isWorkspaceSpec = rawSpec.startsWith('workspace:')
  const parsed = parseDependencySpec(project.rootDir, depName, rawSpec, isWorkspaceSpec)
  if (!parsed) return ''

  if (parsed.spec.type === 'directory') {
    return resolveDirectoryDependency(parsed.spec.fetchSpec, project.rootDir, ctx)
  }
  if (parsed.spec.type !== 'version' && parsed.spec.type !== 'range') return ''

  return resolveVersionDependency({
    depName: parsed.depName,
    rawSpec: parsed.rawSpec,
    isWorkspaceSpec,
    ctx,
  })
}

function parseDependencySpec (
  projectRootDir: string,
  depName: string,
  rawSpec: string,
  isWorkspaceSpec: boolean
): { depName: string, rawSpec: string, spec: { fetchSpec: string, type: string } } | undefined {
  try {
    if (isWorkspaceSpec) {
      const npmSpec = workspacePrefToNpm(rawSpec)
      if (isRelativePathSpec(npmSpec)) {
        rawSpec = npmSpec
      } else {
        ({ depName, rawSpec } = parseRegistrySpec(depName, npmSpec))
      }
    } else if (rawSpec.startsWith('npm:')) {
      ({ depName, rawSpec } = parseRegistrySpec(depName, rawSpec))
    }
    const spec = npa.resolve(depName, rawSpec, projectRootDir)
    return { depName, rawSpec, spec }
  } catch {
    return undefined
  }
}

function resolveDirectoryDependency (fetchSpec: string, projectRootDir: string, ctx: GraphResolutionContext): string {
  ctx.projectMapByDir ??= getProjectMapByDir(ctx.projectMapValues)
  const resolvedPath = path.resolve(projectRootDir, fetchSpec)
  const found = ctx.projectMapByDir[resolvedPath]
  if (found) return found.rootDir

  const matchedProject = ctx.projectMapValues.find(project => path.relative(project.rootDir, fetchSpec) === '')
  if (matchedProject == null) return ''
  ctx.projectMapByDir[resolvedPath] = matchedProject
  return matchedProject.rootDir
}

interface VersionDependencyParams {
  depName: string
  rawSpec: string
  isWorkspaceSpec: boolean
  ctx: GraphResolutionContext
}

function resolveVersionDependency ({
  depName,
  rawSpec,
  isWorkspaceSpec,
  ctx,
}: VersionDependencyParams): string {
  ctx.projectMapByManifestName ??= getProjectMapByManifestName(ctx.projectMapValues)
  const candidates = ctx.projectMapByManifestName.get(depName)
  if (!candidates || candidates.length === 0) return ''

  const versions = candidates.filter(({ manifest }) => manifest.version)
    .map(candidate => candidate.manifest.version) as string[]

  if (ctx.linkWorkspacePackages === false && !isWorkspaceSpec) {
    ctx.unmatched.push({ pkgName: depName, range: rawSpec })
    return ''
  }
  return matchCandidateByVersion({
    candidates,
    depName,
    rawSpec,
    versions,
    isWorkspaceSpec,
    unmatched: ctx.unmatched,
  })
}

interface MatchCandidateParams {
  candidates: BaseProject[]
  depName: string
  rawSpec: string
  versions: string[]
  isWorkspaceSpec: boolean
  unmatched: Array<{ pkgName: string, range: string }>
}

function matchCandidateByVersion ({
  candidates,
  depName,
  rawSpec,
  versions,
  isWorkspaceSpec,
  unmatched,
}: MatchCandidateParams): string {
  if (isWorkspaceSpec && versions.length === 0) {
    const matchedProject = candidates.find(candidate => candidate.manifest.name === depName)
    return matchedProject?.rootDir ?? ''
  }
  if (versions.includes(rawSpec)) {
    const matchedProject = candidates.find(candidate => candidate.manifest.name === depName && candidate.manifest.version === rawSpec)
    return matchedProject?.rootDir ?? ''
  }
  const matched = resolveWorkspaceRange(rawSpec, versions)
  if (!matched) {
    unmatched.push({ pkgName: depName, range: rawSpec })
    return ''
  }
  const matchedProject = candidates.find(candidate => candidate.manifest.name === depName && candidate.manifest.version === matched)
  return matchedProject?.rootDir ?? ''
}

/**
 * The package an `npm:` alias points at and the selector it asks for, read
 * the way the npm resolver reads them. A spec the resolver does not claim
 * comes back with `depName` and the spec unchanged. Throws what
 * `parseBareSpecifier` throws, such as for a registry revision the resolver
 * rejects.
 */
function parseRegistrySpec (depName: string, npmSpec: string): { depName: string, rawSpec: string } {
  const parsed = parseBareSpecifier(npmSpec, depName, 'latest', '')
  return parsed ? { depName: parsed.name, rawSpec: parsed.fetchSpec } : { depName, rawSpec: npmSpec }
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

function getProjectMapByManifestName (projectMapValues: BaseProject[]): Map<string, BaseProject[]> {
  const projectMapByManifestName = new Map<string, BaseProject[]>()
  for (const project of projectMapValues) {
    const { name } = project.manifest
    if (!name) continue
    const projects = projectMapByManifestName.get(name)
    if (projects == null) {
      projectMapByManifestName.set(name, [project])
    } else {
      projects.push(project)
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
