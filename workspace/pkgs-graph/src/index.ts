import '@total-typescript/ts-reset'

import path from 'node:path'

import mapValues from 'ramda/src/map'

import npa from '@pnpm/npm-package-arg'
import type { Package } from '@pnpm/types'
import { parsePref, workspacePrefToNpm } from '@pnpm/npm-resolver'
import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'

export function createPkgGraph<T>(
  pkgs: Array<Package & T>,
  opts?: {
    ignoreDevDeps?: boolean | undefined
    linkWorkspacePackages?: boolean | undefined
  } | undefined
): {
    graph: Record<string, { dependencies: string[]; package: Package; }>
    unmatched: Array<{ pkgName: string; range: string }>
  } {
  const pkgMap = createPkgMap(pkgs)

  const pkgMapValues = Object.values(pkgMap)

  let pkgMapByManifestName: Record<string, Package[] | undefined> | undefined

  let pkgMapByDir: Record<string, Package | undefined> | undefined

  const unmatched: Array<{ pkgName: string; range: string }> = []

  const graph = mapValues(
    (pkg: Package): {
      dependencies: string[];
      package: Package;
    } => ({
      dependencies: createNode(pkg),
      package: pkg,
    }),
    pkgMap
  )

  return { graph, unmatched }

  function createNode(pkg: Package): string[] {
    const dependencies = {
      ...(!opts?.ignoreDevDeps && pkg.manifest?.devDependencies),
      ...pkg.manifest?.optionalDependencies,
      ...pkg.manifest?.dependencies,
    }

    return Object.entries(dependencies)
      .map(([depName, rawSpec]: [string, string]): string | undefined => {
        let spec: { fetchSpec: string; type: string } | undefined

        const isWorkspaceSpec = rawSpec.startsWith('workspace:')

        try {
          if (isWorkspaceSpec) {
            const pref = parsePref(
              workspacePrefToNpm(rawSpec),
              depName,
              'latest',
              ''
            )

            if (pref !== null) {
              rawSpec = pref.fetchSpec
              depName = pref.name
            }
          }

          spec = npa.resolve(depName, rawSpec, pkg.dir)
        } catch (err: unknown) {
          console.error(err)

          return ''
        }

        if (spec?.type === 'directory') {
          pkgMapByDir ??= getPkgMapByDir(pkgMapValues)

          const resolvedPath = path.resolve(pkg.dir, spec.fetchSpec)

          const found = pkgMapByDir[resolvedPath]

          if (found) {
            return found.dir
          }

          // Slow path; only needed when there are case mismatches on case-insensitive filesystems.
          const matchedPkg = pkgMapValues.find(
            (pkg: Package & T): boolean => {
              return path.relative(pkg.dir, spec.fetchSpec) === '';
            }
          )

          if (matchedPkg == null) {
            return ''
          }

          pkgMapByDir[resolvedPath] = matchedPkg

          return matchedPkg.dir
        }

        if (spec?.type !== 'version' && spec?.type !== 'range') {
          return ''
        }

        pkgMapByManifestName ??= getPkgMapByManifestName(pkgMapValues)

        const pkgs = pkgMapByManifestName[depName]

        if (!pkgs || pkgs.length === 0) {
          return ''
        }

        const versions = pkgs
          .filter(({ manifest }: Package): boolean => {
            return typeof manifest?.version !== 'undefined';
          })
          .map((pkg: Package): string | undefined => {
            return pkg.manifest?.version;
          }).filter(Boolean)

        // explicitly check if false, backwards-compatibility (can be undefined)
        const strictWorkspaceMatching =
          opts?.linkWorkspacePackages === false && !isWorkspaceSpec

        if (strictWorkspaceMatching) {
          unmatched.push({ pkgName: depName, range: rawSpec })

          return ''
        }

        if (isWorkspaceSpec && versions.length === 0) {
          const matchedPkg = pkgs.find((pkg) => pkg.manifest?.name === depName)

          return matchedPkg?.dir
        }

        if (versions.includes(rawSpec)) {
          const matchedPkg = pkgs.find(
            (pkg: Package): boolean => {
              return pkg.manifest?.name === depName && pkg.manifest.version === rawSpec;
            }
          )

          return matchedPkg?.dir
        }

        const matched = resolveWorkspaceRange(rawSpec, versions)

        if (!matched) {
          unmatched.push({ pkgName: depName, range: rawSpec })

          return ''
        }

        const matchedPkg = pkgs.find(
          (pkg: Package): boolean => {
            return pkg.manifest?.name === depName && pkg.manifest.version === matched;
          }
        )

        return matchedPkg?.dir
      })
      .filter(Boolean)
  }
}

function createPkgMap<T>(pkgs: (Package & T)[]): Record<string, Package & T> {
  const pkgMap: Record<string, Package & T> = {}

  for (const pkg of pkgs) {
    pkgMap[pkg.dir] = pkg
  }

  return pkgMap
}

function getPkgMapByManifestName(pkgMapValues: Package[]): Record<string, Package[] | undefined> {
  const pkgMapByManifestName: Record<string, Package[] | undefined> = {}

  for (const pkg of pkgMapValues) {
    if (pkg.manifest?.name) {
      ;(pkgMapByManifestName[pkg.manifest.name] ??= []).push(pkg)
    }
  }

  return pkgMapByManifestName
}

function getPkgMapByDir(pkgMapValues: Package[]): Record<string, Package | undefined> {
  const pkgMapByDir: Record<string, Package | undefined> = {}

  for (const pkg of pkgMapValues) {
    pkgMapByDir[path.resolve(pkg.dir)] = pkg
  }

  return pkgMapByDir
}
