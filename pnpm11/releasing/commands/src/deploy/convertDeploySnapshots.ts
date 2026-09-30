import path from 'node:path'
import url from 'node:url'

import * as dp from '@pnpm/deps.path'
import type {
  CustomResolution,
  DirectoryResolution,
  LockfileResolution,
  PackageSnapshot,
  ProjectSnapshot,
  ResolvedDependencies,
  TarballResolution,
} from '@pnpm/lockfile.types'
import type { DepPath, Project } from '@pnpm/types'
import normalizePath from 'normalize-path'

export type DeployAllProjects = Array<Pick<Project, 'manifest' | 'rootDir' | 'rootDirRealPath'>>

export interface ConvertOptions {
  allProjects: DeployAllProjects
  deployDir: string
  deployedProjectRealPath: string
  projectRootDirRealPath: string
  lockfileDir: string
}

export function convertPackageSnapshot (inputSnapshot: PackageSnapshot, opts: ConvertOptions): PackageSnapshot {
  return {
    ...inputSnapshot,
    resolution: convertResolution(inputSnapshot.resolution, opts),
    dependencies: convertResolvedDependencies(inputSnapshot.dependencies, opts),
    optionalDependencies: convertResolvedDependencies(inputSnapshot.optionalDependencies, opts),
  }
}

function convertResolution (
  inputResolution: LockfileResolution,
  opts: Pick<ConvertOptions, 'deployDir' | 'lockfileDir'>
): LockfileResolution {
  if ('integrity' in inputResolution) return inputResolution
  if ('tarball' in inputResolution && typeof inputResolution.tarball === 'string') {
    return convertTarballResolution(inputResolution, inputResolution.tarball, opts)
  }
  if (inputResolution.type === 'directory') {
    const dirResolution = inputResolution as DirectoryResolution
    const resolvedPath = path.resolve(opts.lockfileDir, dirResolution.directory)
    const directory = normalizePath(path.relative(opts.deployDir, resolvedPath))
    return { ...dirResolution, directory }
  }
  if (inputResolution.type === 'git' || inputResolution.type === 'variations') return inputResolution
  // Custom resolution type - pass through as-is
  if (inputResolution.type && typeof inputResolution.type === 'string') return inputResolution
  throw new Error(`Unknown resolution type: ${JSON.stringify(inputResolution)}`)
}

function convertTarballResolution (
  inputResolution: CustomResolution | TarballResolution,
  tarball: string,
  opts: Pick<ConvertOptions, 'deployDir' | 'lockfileDir'>
): LockfileResolution {
  if (!tarball.startsWith('file:')) return { ...inputResolution }
  const inputPath = tarball.slice('file:'.length)
  const resolvedPath = path.resolve(opts.lockfileDir, inputPath)
  const outputPath = normalizePath(path.relative(opts.deployDir, resolvedPath))
  const hasPath = 'path' in inputResolution && typeof inputResolution.path === 'string'
  return {
    ...inputResolution,
    tarball: `file:${outputPath}`,
    ...(hasPath ? { path: outputPath } : {}),
  }
}

export function convertProjectSnapshotToPackageSnapshot (projectSnapshot: ProjectSnapshot, opts: ConvertOptions): PackageSnapshot {
  const resolution: DirectoryResolution = {
    type: 'directory',
    directory: normalizePath(path.relative(opts.deployDir, opts.projectRootDirRealPath)),
  }
  const dependencies = convertResolvedDependencies(projectSnapshot.dependencies, opts)
  const optionalDependencies = convertResolvedDependencies(projectSnapshot.optionalDependencies, opts)
  return {
    dependencies,
    optionalDependencies,
    resolution,
  }
}

type ConvertResolvedDependenciesOptions = Pick<ConvertOptions, 'allProjects' | 'deployedProjectRealPath' | 'lockfileDir' | 'projectRootDirRealPath'>

export function convertResolvedDependencies (
  input: ResolvedDependencies | undefined,
  opts: ConvertResolvedDependenciesOptions
): ResolvedDependencies | undefined {
  if (!input) return undefined
  const output: ResolvedDependencies = {}
  for (const key in input) {
    output[key] = convertResolvedDependency(key, input[key], opts)
  }
  return output
}

function convertResolvedDependency (alias: string, version: string, opts: ConvertResolvedDependenciesOptions): string {
  // A link into the declaring package is resolved where that package is
  // placed, so it reads the same in the deploy directory.
  if (dp.packageRootLinkTarget(version) != null) return version
  const resolveResult = resolveLinkOrFile(version, opts)
  if (!resolveResult) return version
  if (resolveResult.resolvedPath === opts.deployedProjectRealPath) {
    return 'link:.' // the path is relative to the lockfile dir, which means '.' would reference the deploy dir
  }
  resolveResult.packageName ??= alias
  return createFileUrlDepPath(resolveResult, opts.allProjects)
}

export interface ResolveLinkOrFileResult {
  scheme: 'link:' | 'file:'
  resolvedPath: string
  suffix?: string
  packageName?: string
}

export function resolveLinkOrFile (pkgVer: string, opts: Pick<ConvertOptions, 'lockfileDir' | 'projectRootDirRealPath'>): ResolveLinkOrFileResult | undefined {
  const { lockfileDir, projectRootDirRealPath } = opts

  function resolveScheme (scheme: ResolveLinkOrFileResult['scheme'], base: string): ResolveLinkOrFileResult | undefined {
    if (!pkgVer.startsWith(scheme)) return undefined
    const { id, peerDepGraphHash: suffix } = dp.parseDepPath(pkgVer.slice(scheme.length))
    const resolvedPath = path.resolve(base, id)
    return { scheme, resolvedPath, suffix }
  }

  const resolveSchemeResult = resolveScheme('file:', lockfileDir) ?? resolveScheme('link:', projectRootDirRealPath)
  if (resolveSchemeResult) return resolveSchemeResult

  const { name, nonSemverVersion, patchHash, peerDepGraphHash, version } = dp.parse(pkgVer)
  if (!nonSemverVersion) return undefined

  if (version) {
    throw new Error(`Something goes wrong, version should be undefined but isn't: ${version}`)
  }

  const parseResult = resolveLinkOrFile(nonSemverVersion, opts)
  if (!parseResult) return undefined

  if (parseResult.suffix) {
    throw new Error(`Something goes wrong, suffix should be undefined but isn't: ${parseResult.suffix}`)
  }

  parseResult.suffix = `${patchHash ?? ''}${peerDepGraphHash ?? ''}`
  parseResult.packageName = name

  return parseResult
}

export function createFileUrlDepPath (
  { resolvedPath, suffix, packageName }: Pick<ResolveLinkOrFileResult, 'resolvedPath' | 'suffix' | 'packageName'>,
  allProjects: DeployAllProjects
): DepPath {
  const depFileUrl = url.pathToFileURL(resolvedPath).toString()
  const project = allProjects.find(project => project.rootDirRealPath === resolvedPath)
  const name = project?.manifest.name ?? packageName ?? path.basename(resolvedPath)
  return `${name}@${depFileUrl}${suffix ?? ''}` as DepPath
}
