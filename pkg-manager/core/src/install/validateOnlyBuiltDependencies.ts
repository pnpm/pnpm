import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  type DependenciesGraph,
  type DependenciesGraphNode,
} from '@pnpm/resolve-dependencies'

export interface OnlyBuiltDepsValidationOptions {
  onlyBuiltDependencies?: string[]
  strictOnlyBuiltDependencies?: boolean
  lockfileDir: string
}

export async function validateOnlyBuiltDependencies (
  graph: DependenciesGraph,
  opts: OnlyBuiltDepsValidationOptions
): Promise<void> {
  const { onlyBuiltDependencies, strictOnlyBuiltDependencies, lockfileDir } = opts
  if (!onlyBuiltDependencies?.length) return

  const byName = groupByName(graph)

  const unused: Array<{
    entry: string
    matches: Array<{ name: string, version: string, dir: string }>
  }> = []

  const entries = onlyBuiltDependencies.slice()
  const perEntryResults = await Promise.all(entries.map(async (entry) => {
    const name = extractName(entry)
    if (!name) return null

    const nodes = byName.get(name)
    if (!nodes?.length) return null

    const nodeResults = await Promise.all(
      nodes.map(async (node) => {
        const manifest = await readManifest(node)
        if (manifest == null) return null

        const scripts = manifest.scripts
        const hasScripts = Boolean(scripts && hasLifecycleScript(scripts ?? {}))
        return {
          node,
          hasScripts,
        }
      })
    )

    const filtered = nodeResults.filter((r) => r != null) as Array<{ node: DependenciesGraphNode, hasScripts: boolean }>
    if (!filtered.length) return null

    const anyWithScripts = filtered.some(({ hasScripts }) => hasScripts)
    const withoutScripts = filtered
      .filter(({ hasScripts }) => !hasScripts)
      .map(({ node }) => ({ name: node.name, version: node.version, dir: node.dir }))

    if (!anyWithScripts && withoutScripts.length > 0) {
      return { entry, matches: withoutScripts }
    }
    return null
  }))

  for (const res of perEntryResults) {
    if (res) unused.push(res)
  }

  if (!unused.length) return

  const body = formatUnused(unused)
  const message = [
    'The following entries in onlyBuiltDependencies allow packages to run lifecycle scripts, but the resolved packages currently define no lifecycle scripts.',
    'This may leave dormant privileges that could be abused if those packages add scripts in the future.',
    '',
    body,
  ].join('\n')

  if (strictOnlyBuiltDependencies) {
    throw new PnpmError(
      'STRICT_ONLY_BUILT_DEPENDENCIES',
      `${message}\n\nEither remove these entries from onlyBuiltDependencies or intentionally add lifecycle scripts that you expect to run.`
    )
  }

  logger.warn({
    message,
    prefix: lockfileDir,
  })
}

function groupByName (graph: DependenciesGraph): Map<string, DependenciesGraphNode[]> {
  const map = new Map<string, DependenciesGraphNode[]>()
  for (const node of Object.values(graph)) {
    if (!node.name) continue
    let list = map.get(node.name)
    if (!list) {
      list = []
      map.set(node.name, list)
    }
    list.push(node)
  }
  return map
}

function extractName (spec: string): string | null {
  if (!spec) return null
  if (!spec.includes('@')) return spec
  if (spec.startsWith('@')) {
    const secondAt = spec.indexOf('@', 1)
    return secondAt === -1 ? spec : spec.slice(0, secondAt)
  }
  return spec.split('@', 1)[0]!
}

async function readManifest (node: DependenciesGraphNode): Promise<any | null> { // eslint-disable-line @typescript-eslint/no-explicit-any
  try {
    return await safeReadProjectManifestOnly(node.dir)
  } catch {
    // If the manifest cannot be read (e.g. optional dep skipped on this platform),
    // treat it as not installed for validation purposes.
    return null
  }
}

function hasLifecycleScript (scripts: Record<string, string | undefined>): boolean {
  return Boolean(
    scripts.preinstall ??
    scripts.install ??
    scripts.postinstall ??
    scripts.prepare ??
    scripts.prepublish ??
    scripts.prepack
  )
}

function formatUnused (
  unused: Array<{ entry: string, matches: Array<{ name: string, version: string, dir: string }> }>
): string {
  return unused.map(({ entry, matches }) => {
    const lines = matches.map(m => `  - ${m.name}@${m.version} at ${m.dir}`)
    return [`• ${entry}`, ...lines].join('\n')
  }).join('\n\n')
}
