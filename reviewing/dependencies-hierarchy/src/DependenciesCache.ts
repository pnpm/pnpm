import { PackageNode } from './PackageNode'

export class DependenciesCache {
  private readonly dependenciesCache = new Map<string, PackageNode[]>()

  public get (packageAbsolutePath: string): PackageNode[] | undefined {
    return this.dependenciesCache.get(packageAbsolutePath)
  }

  public set (packageAbsolutePath: string, dependencies: PackageNode[]): void {
    this.dependenciesCache.set(packageAbsolutePath, dependencies)
  }
}
