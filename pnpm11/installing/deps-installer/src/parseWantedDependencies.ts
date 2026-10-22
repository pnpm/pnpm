import type { Catalog } from '@pnpm/catalogs.types'
import { hasAlias, type ManifestWantedDependency, type WantedDependency } from '@pnpm/installing.deps-resolver'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { Dependencies } from '@pnpm/types'
import semver from 'semver'

export interface KeptRangeConflict {
  alias: string
  requested: string
  kept: string
}

export interface ParsedWantedDependencies {
  wantedDependencies: WantedDependency[]
  /**
   * The selectors dropped because the version they request doesn't satisfy the range the manifest
   * keeps. Those dependencies are left untouched. Only ever non-empty under `readonlyManifest`.
   */
  outsideKeptRange: KeptRangeConflict[]
  /**
   * The selectors resolution can't be trusted to honor — a range or a dist tag, which only names a
   * version once resolution has run — so the specifier the manifest or a hook/override keeps was
   * used instead. Only ever non-empty under `readonlyManifest` or for `readonlySpecifiers`.
   */
  supersededByKeptRange: KeptRangeConflict[]
  /**
   * The aliases dropped because a hook removes them from the manifest it reads, so the project
   * can't declare them. Only ever non-empty for `hookRemovedAliases`.
   */
  removedByHook: string[]
}

interface ParseWantedDependenciesOptions {
  allowNew: boolean
  currentBareSpecifiers: Dependencies
  defaultTag: string
  dev: boolean
  devDependencies: Dependencies
  optional: boolean
  optionalDependencies: Dependencies
  overrides?: Record<string, string>
  updateWorkspaceDependencies?: boolean
  preferredSpecs?: Record<string, string>
  saveCatalogName?: string
  defaultCatalog?: Catalog
  /**
   * The manifest keeps its specifiers, so a requested version is applied only when it satisfies
   * the declared one — the lockfile importer entry has to keep satisfying its own specifier.
   */
  readonlyManifest?: boolean
  readonlySpecifiers?: Dependencies
  /**
   * Aliases a hook deletes from the manifest it reads. Declaring one would leave the lockfile
   * importer holding a dependency the next read drops, which `--frozen-lockfile` rejects.
   */
  hookRemovedAliases?: Set<string>
}

export function parseWantedDependencies (
  rawWantedDependencies: string[],
  opts: ParseWantedDependenciesOptions
): ParsedWantedDependencies {
  const wantedDeps = rawWantedDependencies
    .map((rawWantedDependency) => parseRequestedDependency(rawWantedDependency, opts))
    .filter((wd) => wd !== null)

  if (!opts.readonlyManifest && opts.readonlySpecifiers == null && opts.hookRemovedAliases == null) {
    return { wantedDependencies: wantedDeps, outsideKeptRange: [], supersededByKeptRange: [], removedByHook: [] }
  }
  return applyKeptSpecifiers(wantedDeps, opts)
}

function parseRequestedDependency (rawWantedDependency: string, opts: ParseWantedDependenciesOptions) {
  const parsed = parseWantedDependency(rawWantedDependency)
  const alias = parsed['alias']
  const hasReadonlySpecifier = alias != null &&
    opts.readonlySpecifiers != null &&
    Object.hasOwn(opts.readonlySpecifiers, alias)

  if (!opts.allowNew && !isAlreadyDeclared({ alias, hasReadonlySpecifier, opts })) {
    return null
  }
  const bareSpecifier = pickRequestedBareSpecifier({ alias, bareSpecifier: parsed['bareSpecifier'], opts })
  const result = {
    alias,
    dev: Boolean(opts.dev || alias && !!getOwnValue(opts.devDependencies, alias)),
    optional: Boolean(opts.optional || alias && !!getOwnValue(opts.optionalDependencies, alias)),
    prevSpecifier: hasReadonlySpecifier ? opts.readonlySpecifiers![alias] : alias && getOwnValue(opts.currentBareSpecifiers, alias),
    saveCatalogName: opts.saveCatalogName,
  } satisfies Partial<WantedDependency>
  return {
    ...result,
    bareSpecifier: bareSpecifier || pickFallbackBareSpecifier(alias, opts),
  }
}

function isAlreadyDeclared (
  { alias, hasReadonlySpecifier, opts }: { alias: string | undefined, hasReadonlySpecifier: boolean, opts: ParseWantedDependenciesOptions }
): boolean {
  if (!alias) return false
  return hasReadonlySpecifier || Object.hasOwn(opts.currentBareSpecifiers, alias)
}

function pickRequestedBareSpecifier (
  { alias, bareSpecifier, opts }: { alias: string | undefined, bareSpecifier: string | undefined, opts: ParseWantedDependenciesOptions }
): string | undefined {
  if (!alias) return bareSpecifier
  if (shouldUseDefaultCatalog({ alias, bareSpecifier, opts })) {
    return 'catalog:'
  }
  return bareSpecifier ?? (getOwnValue(opts.currentBareSpecifiers, alias) || undefined)
}

function shouldUseDefaultCatalog (
  { alias, bareSpecifier, opts }: { alias: string, bareSpecifier: string | undefined, opts: ParseWantedDependenciesOptions }
): boolean {
  const catalogSpecifier = getOwnValue(opts.defaultCatalog, alias)
  if (!catalogSpecifier) return false
  const currentBareSpecifier = getOwnValue(opts.currentBareSpecifiers, alias)
  return (!currentBareSpecifier && bareSpecifier === undefined) ||
    catalogSpecifier === bareSpecifier ||
    catalogSpecifier === currentBareSpecifier
}

function pickFallbackBareSpecifier (alias: string | undefined, opts: ParseWantedDependenciesOptions): string {
  if (!alias) return opts.defaultTag
  return getOwnValue(opts.preferredSpecs, alias) || getOwnValue(opts.overrides, alias) || opts.defaultTag
}

function applyKeptSpecifiers (wantedDeps: WantedDependency[], opts: ParseWantedDependenciesOptions): ParsedWantedDependencies {
  const parsed: ParsedWantedDependencies = {
    wantedDependencies: [],
    outsideKeptRange: [],
    supersededByKeptRange: [],
    removedByHook: [],
  }
  for (const wantedDep of wantedDeps) {
    if (!hasAlias(wantedDep)) {
      parsed.wantedDependencies.push(wantedDep)
      continue
    }
    if (opts.hookRemovedAliases?.has(wantedDep.alias)) {
      parsed.removedByHook.push(wantedDep.alias)
      continue
    }
    if (opts.readonlySpecifiers != null && Object.hasOwn(opts.readonlySpecifiers, wantedDep.alias)) {
      applyReadonlySpecifier(parsed, wantedDep)
      continue
    }
    if (!opts.readonlyManifest) {
      parsed.wantedDependencies.push(wantedDep)
      continue
    }
    applyReadonlyManifestSpecifier(parsed, wantedDep)
  }
  return parsed
}

function applyReadonlySpecifier (parsed: ParsedWantedDependencies, wantedDep: ManifestWantedDependency): void {
  const { bareSpecifier, prevSpecifier } = wantedDep
  if (prevSpecifier == null || bareSpecifier === prevSpecifier) {
    parsed.wantedDependencies.push(wantedDep)
  } else {
    supersedeWithKeptSpecifier(parsed, wantedDep, prevSpecifier)
  }
}

function applyReadonlyManifestSpecifier (parsed: ParsedWantedDependencies, wantedDep: ManifestWantedDependency): void {
  const { alias, bareSpecifier, prevSpecifier } = wantedDep
  if (!prevSpecifier || bareSpecifier === prevSpecifier) {
    parsed.wantedDependencies.push(wantedDep)
  } else if (semver.valid(bareSpecifier) != null && semver.validRange(prevSpecifier) != null) {
    // Both sides are concrete enough to judge now: matching a version against a range is exact.
    if (semver.satisfies(bareSpecifier, prevSpecifier)) {
      parsed.wantedDependencies.push(wantedDep)
    } else {
      parsed.outsideKeptRange.push({ alias, requested: bareSpecifier, kept: prevSpecifier })
    }
  } else {
    // A range, a dist tag, or a kept specifier that isn't a semver range. Nothing here names a
    // version yet, and asking whether one range contains another is not answered consistently
    // across semver implementations — so resolve what the manifest declares, which is the
    // specifier the importer entry will record.
    supersedeWithKeptSpecifier(parsed, wantedDep, prevSpecifier)
  }
}

function supersedeWithKeptSpecifier (parsed: ParsedWantedDependencies, wantedDep: ManifestWantedDependency, kept: string): void {
  parsed.supersededByKeptRange.push({ alias: wantedDep.alias, requested: wantedDep.bareSpecifier, kept })
  parsed.wantedDependencies.push({ ...wantedDep, bareSpecifier: kept })
}

function getOwnValue<Value> (record: Record<string, Value> | undefined, key: string): Value | undefined {
  return record != null && Object.hasOwn(record, key) ? record[key] : undefined
}
