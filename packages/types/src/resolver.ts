// Resolver types - duplicated in @pnpm/hooks.types for pnpmfile hooks
// Users should import from @pnpm/hooks.types for pnpmfile usage
export interface PackageDescriptor {
  name: string
  range: string
  type?: 'prod' | 'dev' | 'optional'
}

export interface ResolveOptions {
  lockfileDir: string
  projectDir: string
  preferredVersions: Record<string, string>
}

export interface ResolveResult {
  id: string
  resolution: any // eslint-disable-line @typescript-eslint/no-explicit-any
  manifest?: {
    name: string
    version: string
    [key: string]: unknown
  }
  resolvedVia: string
  getLockfileResolution?: (resolution: any) => any // eslint-disable-line @typescript-eslint/no-explicit-any
}

export interface ResolverPlugin {
  // Resolution phase: resolve package descriptors
  supportsDescriptor?: (descriptor: PackageDescriptor) => boolean | Promise<boolean>
  resolve?: (descriptor: PackageDescriptor, opts: ResolveOptions) => ResolveResult | Promise<ResolveResult>

  // Headless install phase: convert lockfile entry back to fetchable resolution
  supportsLockfileResolution?: (pkgId: string, lockfileResolution: any) => boolean | Promise<boolean> // eslint-disable-line @typescript-eslint/no-explicit-any
  fromLockfileResolution?: (pkgId: string, lockfileResolution: any, opts: ResolveOptions) => any | Promise<any> // eslint-disable-line @typescript-eslint/no-explicit-any
}
