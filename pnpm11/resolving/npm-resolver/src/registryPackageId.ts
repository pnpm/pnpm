import { pickDeclaredRegistryPrefix } from '@pnpm/config.pick-registry-for-package'
import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'

import type { ResolveFromNpmContext } from './resolverTypes.js'

/**
 * The resolution id, and so the lockfile key, of a package resolved from
 * `registry`: `<name>@<prefix>:<version>` when the registry declares a prefix
 * (see `pickDeclaredRegistryPrefix`), `<name>@<version>` otherwise. Every
 * route to one registry mints one id, while the same name and version from
 * two registries get two.
 */
export function registryPackageId (
  ctx: Pick<ResolveFromNpmContext, 'declaredRegistriesByPrefix'>,
  { name, version, registry }: { name: string, version: string, registry: string }
): PkgResolutionId {
  return qualifiedPackageId(name, version, pickDeclaredRegistryPrefix(ctx.declaredRegistriesByPrefix, registry))
}

export function qualifiedPackageId (name: string, version: string, keyPrefix: string | undefined): PkgResolutionId {
  return (keyPrefix == null ? `${name}@${version}` : `${name}@${keyPrefix}:${version}`) as PkgResolutionId
}
