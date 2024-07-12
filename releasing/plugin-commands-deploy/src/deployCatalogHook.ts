import { matchCatalogResolveResult, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { type Catalogs } from '@pnpm/catalogs.types'
import { type ProjectManifest, DEPENDENCIES_FIELDS } from '@pnpm/types'

/**
 * Teach the `pnpm deploy` command how to interpret the catalog: protocol.
 *
 * This is a hack to work around a design problem between pnpm deploy and
 * catalogs.
 *
 *   - The catalog protocol is intentionally only allowed to be used by
 *     importers. External dependencies cannot use the catalog: protocol by
 *     design.
 *   - When using pnpm deploy, dependency workspace packages aren't considered
 *     "importers".
 *
 * To work around the conflict of designs above, this readPackage hook exists to
 * make catalogs usable by non-importers specifically on pnpm deploy.
 *
 * Unfortunately this introduces a correctness issue where the catalog: protocol
 * is replaced for all packages (even external dependencies), not just packages
 * within the pnpm workspace. This caveat is somewhat mitigated by the fact that
 * a regular pnpm install would still fail before users could pnpm deploy a
 * project.
 */
export function deployCatalogHook (catalogs: Catalogs, pkg: ProjectManifest): ProjectManifest {
  for (const depField of DEPENDENCIES_FIELDS) {
    const depsBlock = pkg[depField]
    if (depsBlock == null) {
      continue
    }

    for (const [alias, pref] of Object.entries(depsBlock)) {
      const resolveResult = resolveFromCatalog(catalogs, { alias, pref })

      matchCatalogResolveResult(resolveResult, {
        unused: () => {},

        misconfiguration: (result) => {
          throw result.error
        },

        found: (result) => {
          depsBlock[alias] = result.resolution.specifier
        },
      })
    }
  }

  return pkg
}
