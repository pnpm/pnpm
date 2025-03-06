import path from 'path'
import url from 'url'
import normalizePath from 'normalize-path'
import pick from 'ramda/src/pick'
import { USEFUL_NON_ROOT_PNPM_FIELDS } from '@pnpm/constants'
import * as dp from '@pnpm/dependency-path'
import {
  type DirectoryResolution,
  type LockfileObject,
  type LockfileResolution,
  type PackageSnapshot,
  type PackageSnapshots,
  type ProjectSnapshot,
  type ResolvedDependencies,
} from '@pnpm/lockfile.types'
import {
  type DependenciesField,
  type DepPath,
  type Project,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'

const DEPENDENCIES_FIELD = ['dependencies', 'devDependencies', 'optionalDependencies'] as const satisfies DependenciesField[]

export interface CreateDeployFilesOptions {
  allProjects: Array<Pick<Project, 'manifest' | 'rootDirRealPath'>>
  deployDir: string
  lockfile: LockfileObject
  lockfileDir: string
  rootProjectManifest?: Pick<ProjectManifest, 'pnpm'>
  selectedProjectManifest: ProjectManifest
  projectId: ProjectId
  rootProjectManifestDir: string
}

export interface DeployFiles {
  lockfile: LockfileObject
  manifest: ProjectManifest
}

export function createDeployFiles ({
  allProjects,
  deployDir,
  lockfile,
  lockfileDir,
  rootProjectManifest,
  selectedProjectManifest,
  projectId,
  rootProjectManifestDir,
}: CreateDeployFilesOptions): DeployFiles {
  const deployedProjectRealPath = path.resolve(lockfileDir, projectId)
  const inputSnapshot = lockfile.importers[projectId]

  const targetSnapshot: ProjectSnapshot = {
    ...inputSnapshot,
    specifiers: {},
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
  }

  const targetPackageSnapshots: PackageSnapshots = {}
  for (const name in lockfile.packages) {
    const inputDepPath = name as DepPath
    const inputSnapshot = lockfile.packages[inputDepPath]
    const resolveResult = resolveLinkOrFile(inputDepPath, {
      lockfileDir,
      projectRootDirRealPath: rootProjectManifestDir,
    })
    const outputDepPath = resolveResult
      ? createFileUrlDepPath(resolveResult, allProjects)
      : inputDepPath
    targetPackageSnapshots[outputDepPath] = convertPackageSnapshot(inputSnapshot, {
      allProjects,
      deployDir,
      deployedProjectRealPath,
      lockfileDir,
      projectRootDirRealPath: rootProjectManifestDir,
    })
  }

  for (const importerPath in lockfile.importers) {
    if (importerPath === projectId) continue
    const projectSnapshot = lockfile.importers[importerPath as ProjectId]
    const projectRootDirRealPath = path.resolve(lockfileDir, importerPath)
    const packageSnapshot = convertProjectSnapshotToPackageSnapshot(projectSnapshot, {
      allProjects,
      deployDir,
      lockfileDir,
      deployedProjectRealPath,
      projectRootDirRealPath,
    })
    const depPath = createFileUrlDepPath({ resolvedPath: projectRootDirRealPath }, allProjects)
    targetPackageSnapshots[depPath] = packageSnapshot
  }

  for (const field of DEPENDENCIES_FIELD) {
    const targetDependencies = targetSnapshot[field] ?? {}
    const targetSpecifiers = targetSnapshot.specifiers
    const inputDependencies = inputSnapshot[field] ?? {}
    for (const name in inputDependencies) {
      const version = inputDependencies[name]
      const resolveResult = resolveLinkOrFile(version, {
        lockfileDir,
        projectRootDirRealPath: path.resolve(lockfileDir, projectId),
      })

      if (!resolveResult) {
        targetSpecifiers[name] = targetDependencies[name] = version
        continue
      }

      targetSpecifiers[name] = targetDependencies[name] =
        resolveResult.resolvedPath === deployedProjectRealPath ? 'link:.' : createFileUrlDepPath(resolveResult, allProjects)
    }
  }

  const result: DeployFiles = {
    lockfile: {
      ...lockfile,
      overrides: undefined, // the effects of package overrides should already be part of the package snapshots
      patchedDependencies: undefined,
      packageExtensionsChecksum: undefined, // the effects of the package extensions should already be part of the package snapshots
      pnpmfileChecksum: undefined, // the effects of the pnpmfile should already be part of the package snapshots
      importers: {
        ['.' as ProjectId]: targetSnapshot,
      },
      packages: targetPackageSnapshots,
    },
    manifest: {
      ...selectedProjectManifest,
      dependencies: targetSnapshot.dependencies,
      devDependencies: targetSnapshot.devDependencies,
      optionalDependencies: targetSnapshot.optionalDependencies,
      pnpm: {
        ...rootProjectManifest?.pnpm,
        ...pick(USEFUL_NON_ROOT_PNPM_FIELDS, selectedProjectManifest.pnpm ?? {}),
        overrides: undefined, // the effects of package overrides should already be part of the package snapshots
        patchedDependencies: undefined,
        packageExtensions: undefined, // the effects of the package extensions should already be part of the package snapshots
      },
    },
  }

  if (lockfile.patchedDependencies) {
    result.lockfile.patchedDependencies = {}
    result.manifest.pnpm!.patchedDependencies = {}

    for (const name in lockfile.patchedDependencies) {
      const patchInfo = lockfile.patchedDependencies[name]
      const resolvedPath = path.resolve(rootProjectManifestDir, patchInfo.path)
      const relativePath = normalizePath(path.relative(deployDir, resolvedPath))
      result.manifest.pnpm!.patchedDependencies[name] = relativePath
      result.lockfile.patchedDependencies[name] = {
        hash: patchInfo.hash,
        path: relativePath,
      }
    }
  }

  return result
}

interface ConvertOptions {
  allProjects: CreateDeployFilesOptions['allProjects']
  deployDir: string
  deployedProjectRealPath: string
  projectRootDirRealPath: string
  lockfileDir: string
}

function convertPackageSnapshot (inputSnapshot: PackageSnapshot, opts: ConvertOptions): PackageSnapshot {
  const inputResolution = inputSnapshot.resolution
  let outputResolution: LockfileResolution
  if ('integrity' in inputResolution) {
    outputResolution = inputResolution
  } else if ('tarball' in inputResolution) {
    outputResolution = { ...inputResolution }
    if (inputResolution.tarball.startsWith('file:')) {
      const inputPath = inputResolution.tarball.slice('file:'.length)
      const resolvedPath = path.resolve(opts.lockfileDir, inputPath)
      const outputPath = normalizePath(path.relative(opts.deployDir, resolvedPath))
      outputResolution.tarball = `file:${outputPath}`
      if (inputResolution.path) outputResolution.path = outputPath
    }
  } else if (inputResolution.type === 'directory') {
    const resolvedPath = path.resolve(opts.lockfileDir, inputResolution.directory)
    const directory = normalizePath(path.relative(opts.deployDir, resolvedPath))
    outputResolution = { ...inputResolution, directory }
  } else if (inputResolution.type === 'git') {
    outputResolution = inputResolution
  } else {
    const resolution: never = inputResolution // `never` is the type guard to force fixing this code when adding new type of resolution
    throw new Error(`Unknown resolution type: ${JSON.stringify(resolution)}`)
  }

  return {
    ...inputSnapshot,
    resolution: outputResolution,
    dependencies: convertResolvedDependencies(inputSnapshot.dependencies, opts),
    optionalDependencies: convertResolvedDependencies(inputSnapshot.optionalDependencies, opts),
  }
}

function convertProjectSnapshotToPackageSnapshot (projectSnapshot: ProjectSnapshot, opts: ConvertOptions): PackageSnapshot {
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

function convertResolvedDependencies (
  input: ResolvedDependencies | undefined,
  opts: Pick<ConvertOptions, 'allProjects' | 'deployedProjectRealPath' | 'lockfileDir' | 'projectRootDirRealPath'>
): ResolvedDependencies | undefined {
  if (!input) return undefined
  const output: ResolvedDependencies = {}

  for (const key in input) {
    const version = input[key]
    const resolveResult = resolveLinkOrFile(version, opts)
    if (!resolveResult) {
      output[key] = version
      continue
    }

    if (resolveResult.resolvedPath === opts.deployedProjectRealPath) {
      output[key] = 'link:.' // the path is relative to the lockfile dir, which means '.' would reference the deploy dir
      continue
    }

    output[key] = createFileUrlDepPath(resolveResult, opts.allProjects)
  }

  return output
}

interface ResolveLinkOrFileResult {
  scheme: 'link:' | 'file:'
  resolvedPath: string
  suffix?: string
}

function resolveLinkOrFile (pkgVer: string, opts: Pick<ConvertOptions, 'lockfileDir' | 'projectRootDirRealPath'>): ResolveLinkOrFileResult | undefined {
  const { lockfileDir, projectRootDirRealPath } = opts

  function resolveScheme (scheme: ResolveLinkOrFileResult['scheme'], base: string): ResolveLinkOrFileResult | undefined {
    if (!pkgVer.startsWith(scheme)) return undefined
    const { id, peersSuffix: suffix } = dp.parseDepPath(pkgVer.slice(scheme.length))
    const resolvedPath = path.resolve(base, id)
    return { scheme, resolvedPath, suffix }
  }

  const resolveSchemeResult = resolveScheme('file:', lockfileDir) ?? resolveScheme('link:', projectRootDirRealPath)
  if (resolveSchemeResult) return resolveSchemeResult

  const { nonSemverVersion, patchHash, peersSuffix, version } = dp.parse(pkgVer)
  if (!nonSemverVersion) return undefined

  if (version) {
    throw new Error(`Something goes wrong, version should be undefined but isn't: ${version}`)
  }

  const parseResult = resolveLinkOrFile(nonSemverVersion, opts)
  if (!parseResult) return undefined

  if (parseResult.suffix) {
    throw new Error(`Something goes wrong, suffix should be undefined but isn't: ${parseResult.suffix}`)
  }

  parseResult.suffix = `${patchHash ?? ''}${peersSuffix ?? ''}`

  return parseResult
}

function createFileUrlDepPath (
  { resolvedPath, suffix }: Pick<ResolveLinkOrFileResult, 'resolvedPath' | 'suffix'>,
  allProjects: CreateDeployFilesOptions['allProjects']
): DepPath {
  const depFileUrl = url.pathToFileURL(resolvedPath).toString()
  const project = allProjects.find(project => project.rootDirRealPath === resolvedPath)
  const name = project?.manifest.name ?? path.basename(resolvedPath)
  return `${name}@${depFileUrl}${suffix ?? ''}` as DepPath
}
