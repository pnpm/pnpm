import type { Ord } from 'ramda'
import path from 'ramda/src/path'
import sortBy from 'ramda/src/sortBy'

import {
  type PackageInfo,
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type PackageJsonListItem,
  type RenderJsonResultItem,
  type PackageDependencyHierarchy,
} from '@pnpm/types'
import type { PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'

import { getPkgInfo } from './getPkgInfo.js'

const sortPackages = sortBy.default(path.default(['pkg', 'alias']) as (pkg: PackageNode | PackageInfo) => Ord)

export async function renderJson(
  pkgs: PackageDependencyHierarchy[],
  opts: {
    depth: number
    long: boolean
    search: boolean
  }
): Promise<string> {
  const jsonArr = await Promise.all(
    pkgs.map(async (pkg: PackageDependencyHierarchy): Promise<RenderJsonResultItem> => {
      const jsonObj: RenderJsonResultItem = {
        name: pkg.name,
        version: pkg.version,
        path: pkg.path,
        private: !!pkg.private,
      }

      Object.assign(
        jsonObj,
        Object.fromEntries(
          await Promise.all(
            ([...DEPENDENCIES_FIELDS.sort(), 'unsavedDependencies'] as const)
              .filter((dependenciesField): boolean => {
                return Boolean(pkg[dependenciesField]?.length);
              })
              .map(async (dependenciesField: 'unsavedDependencies' | DependenciesField): Promise<(DependenciesField | 'unsavedDependencies' | Record<string, PackageJsonListItem>)[]> => {
                return [
                  dependenciesField,
                  await toJsonResult(pkg[dependenciesField] ?? [], {
                    long: opts.long,
                  }),
                ];
              })
          )
        )
      )

      return jsonObj
    })
  )

  return JSON.stringify(jsonArr, null, 2)
}

export async function toJsonResult(
  entryNodes: PackageNode[] | PackageInfo[],
  opts: {
    long: boolean
  }
): Promise<Record<string, PackageJsonListItem>> {
  const dependencies: Record<string, PackageJsonListItem> = {}
  await Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const subDependencies = await toJsonResult(node.dependencies ?? [], opts)
      const dep: PackageJsonListItem = opts.long
        ? await getPkgInfo(node)
        : {
          alias: node.alias as string | undefined,
          from: node.name,
          version: node.version,
          resolved: node.resolved,
          path: node.path,
        }
      if (Object.keys(subDependencies).length > 0) {
        dep.dependencies = subDependencies
      }
      if (!dep.resolved) {
        delete dep.resolved
      }
      // @ts-ignore
      delete dep.alias

      dependencies[node.alias] = dep
    })
  )

  return dependencies
}
