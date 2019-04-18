import { Lockfile } from '@pnpm/lockfile-types'
import {
  Dependencies as StrictDependencies,
  PackageManifest,
} from '@pnpm/types'
interface PackageJson extends PackageManifest {
  dependencies: StrictDependencies
  devDependencies: StrictDependencies
  optionalDependencies: StrictDependencies
  peerDependencies: StrictDependencies
}
export { PackageJson, StrictDependencies }
export interface Logger {
  log (msg: string): void
}

export type DependencyType =
  | 'dependencies'
  | 'devDependencies'
  | 'optionalDependencies'
  | 'peerDependencies'
export interface NullableDependencies {
  [name: string]: string | null
}
export interface DependencyMap {
  dependencies?: NullableDependencies
  devDependencies?: NullableDependencies
  optionalDependencies?: NullableDependencies
  peerDependencies?: NullableDependencies
}

export interface HookContext extends Logger {
}
export type ReadPackageHook = (pkg: PackageJson, context: HookContext) => PackageJson
export type AfterAllResolvedHook = (lockfile: Lockfile, context: HookContext) => Lockfile

export interface PnpmHooks {
  readPackage?: ReadPackageHook
  afterAllResolved?: AfterAllResolvedHook
}

export interface ReadPackageUtils {
  /**
   * @param dependency The dependency name.
   * @param target The target version, must be a valid string.
   * @param type The dependency type, defaults to `dependencies`.
   * @throws If `target` isn't a string or `type` isn't a valid dependency type.
   */
  setDependency (dependency: string, target: string, type?: DependencyType): void

  /**
   * Set multiple dependencies at once
   * @param dependencyMap
   * @param type
   * @throws If `type` isn't a valid dependency type.
   */
  setDependencies (
    dependencyMap: NullableDependencies,
    type: DependencyType,
  ): void

  /**
   * @param dependencyMap
   */
  setDependencies (dependencyMap: DependencyMap): void

  /**
   * @param dependency The dependency name.
   * @param type The dependency type.
   * @throws If `type` isn't a valid dependency type.
   */
  removeDependency (dependency: string, type?: DependencyType): void

  /**
   * Appends the message to the internal list of messages to be printed
   * as part of the `logChanges()` call.
   */
  log (message?: unknown, ...optionalParams: unknown[]): void

  /**
   * @returns `true` if there were changes to be logged, `false` otherwise.
   */
  logChanges (): boolean
}
