import '@total-typescript/ts-reset'

import type { DependenciesField, DirectDep, Lockfile, LockfileWalkerStep, PackageSnapshot } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'

export function lockfileWalkerGroupImporterSteps(
  lockfile: Lockfile,
  importerIds: string[],
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean } | undefined
    skipped?: Set<string> | undefined
  } | undefined
): {
    importerId: string;
    step: LockfileWalkerStep;
  }[] {
  const walked = new Set<string>(
    typeof opts?.skipped === 'undefined' ? [] : Array.from(opts.skipped)
  )

  return importerIds.map((importerId) => {
    const projectSnapshot = lockfile.importers[importerId]
    const entryNodes = Object.entries({
      ...(opts?.include?.devDependencies === false
        ? {}
        : projectSnapshot.devDependencies),
      ...(opts?.include?.dependencies === false
        ? {}
        : projectSnapshot.dependencies),
      ...(opts?.include?.optionalDependencies === false
        ? {}
        : projectSnapshot.optionalDependencies),
    })
      .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
      .filter((nodeId) => nodeId !== null) as string[]
    return {
      importerId,
      step: step(
        {
          includeOptionalDependencies:
            opts?.include?.optionalDependencies !== false,
          lockfile,
          walked,
        },
        entryNodes
      ),
    }
  })
}

export function lockfileWalker(
  lockfile: Lockfile,
  importerIds: string[],
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean } | undefined
    skipped?: Set<string> | undefined
  } | undefined
): {
    directDeps: DirectDep[];
    step: LockfileWalkerStep;
  } {
  const walked = new Set<string>(
    opts?.skipped != null ? Array.from(opts?.skipped) : []
  )
  const entryNodes: string[] = []
  const directDeps: Array<DirectDep> = []

  importerIds.forEach((importerId: string): void => {
    const projectSnapshot = lockfile.importers[importerId]

    Object.entries({
      ...(opts?.include?.devDependencies === false
        ? {}
        : projectSnapshot.devDependencies),
      ...(opts?.include?.dependencies === false
        ? {}
        : projectSnapshot.dependencies),
      ...(opts?.include?.optionalDependencies === false
        ? {}
        : projectSnapshot.optionalDependencies),
    }).forEach(([pkgName, reference]: [string, string]): void => {
      const depPath = dp.refToRelative(reference, pkgName)

      if (depPath === null) {
        return
      }

      entryNodes.push(depPath)

      directDeps.push({ alias: pkgName, depPath })
    })
  })

  return {
    directDeps,
    step: step(
      {
        includeOptionalDependencies:
          opts?.include?.optionalDependencies !== false,
        lockfile,
        walked,
      },
      entryNodes
    ),
  }
}

function step(
  ctx: {
    includeOptionalDependencies: boolean
    lockfile: Lockfile
    walked: Set<string>
  },
  nextDepPaths: string[]
): LockfileWalkerStep {
  const result: LockfileWalkerStep = {
    dependencies: [],
    links: [],
    missing: [],
  }

  for (const depPath of nextDepPaths) {
    if (ctx.walked.has(depPath)) {
      continue
    }

    ctx.walked.add(depPath)

    const pkgSnapshot = ctx.lockfile.packages?.[depPath]

    if (pkgSnapshot == null) {
      if (depPath.startsWith('link:')) {
        result.links.push(depPath)
        continue
      }

      result.missing.push(depPath)

      continue
    }

    result.dependencies.push({
      depPath,
      next: () =>
        step(
          ctx,
          next(
            { includeOptionalDependencies: ctx.includeOptionalDependencies },
            pkgSnapshot
          )
        ),
      pkgSnapshot,
    })
  }

  return result
}

function next(
  opts: { includeOptionalDependencies: boolean },
  nextPkg: PackageSnapshot
): string[] {
  return Object.entries({
    ...nextPkg.dependencies,
    ...(opts.includeOptionalDependencies ? nextPkg.optionalDependencies : {}),
  })
    .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
}
