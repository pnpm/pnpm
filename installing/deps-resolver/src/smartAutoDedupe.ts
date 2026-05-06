import { parse as parseDepPath } from '@pnpm/deps.path'
import type { DepPath } from '@pnpm/types'
import semver from 'semver'

import type { ResolvedPackage } from './resolveDependencies.js'
import type { GenericDependenciesGraphWithResolvedChildren } from './resolvePeers.js'

type DependenciesGraph = GenericDependenciesGraphWithResolvedChildren<ResolvedPackage>

interface Candidate {
  depPath: DepPath
  version: string
}

// Only resolutions of registry-hosted tarballs (HTTP/HTTPS) are
// interchangeable for dedupe. Directory / git / binary / custom /
// variations resolutions are bound to a specific location or platform.
// Local TarballResolutions (`file:./pkg.tgz`) carry no `type` either, so
// we additionally require an http(s) tarball URL — otherwise a local
// tarball could be silently swapped with a registry resolve of the same
// name and version.
function isDedupableResolution (resolution: ResolvedPackage['resolution']): boolean {
  // Every non-tarball Resolution variant carries an explicit `type`.
  if ('type' in resolution && resolution.type !== undefined) return false
  const tarball = (resolution as { tarball?: unknown }).tarball
  return typeof tarball === 'string' && /^https?:\/\//i.test(tarball)
}

// Post-resolution backtracking dedupe pass. After all dependencies have
// been resolved into the graph, for every parent → child edge: if a
// higher version of the same package already exists in the graph
// (sharing the same peer set and patch hash, both registry-resolved) and
// that higher version satisfies the original spec range that requested
// this child, rewrite the edge to point at the higher version. Orphaned
// snapshots are subsequently removed by pruneSharedLockfile.
//
// The pass is intentionally conservative:
//   - only registry tarball resolves participate (see
//     isDedupableResolution); local, git, workspace, and platform-variant
//     resolves are skipped on both the source and candidate side.
//   - never crosses peer-dep-graph or patch-hash boundaries
//   - skips specs that are not valid semver ranges (workspace:, file:,
//     git URLs, dist tags)
//   - never downgrades, never picks a version not already present in the
//     graph
//
// Only transitive (parent → child) edges in the graph are rewritten;
// importer-level direct refs (the values in
// `dependenciesByProjectId[importerId]`) are untouched. Rewriting a
// direct ref is a manifest-affecting decision — it changes which version
// is recorded in `pnpm-lock.yaml`'s `importers[*].dependencies` and, on
// `--save`, can flow back into `package.json`. That belongs to the
// explicit `pnpm dedupe` flow, not to a silent post-resolution pass on
// every install. As a side effect this means the lockfile can carry
// (direct@1.0.0, transitive→1.5.0) pairs after the pass; that asymmetry
// is intentional.
//
// Complexity: O(E · K) where E is the number of parent → child edges and
// K is the maximum number of distinct in-graph versions sharing a single
// (name, peer hash, patch hash) group. K is small (typically 1–5) in
// real projects, so the pass is effectively linear in the number of
// edges.
export function applySmartAutoDedupe (graph: DependenciesGraph): void {
  // name → peerDepGraphHash → patchHash → sorted (descending) candidates.
  // Keying on patchHash too is what prevents a patched `foo@1.0.0` from
  // being silently rewritten to an unpatched `foo@1.1.0` (which would
  // drop the patch). Singleton groups (length 1) are removed below so
  // the inner lookup can rely on every Map hit being a real dedupe
  // candidate set.
  const candidates = new Map<string, Map<string, Map<string, Candidate[]>>>()

  for (const depPath of Object.keys(graph) as DepPath[]) {
    const node = graph[depPath]
    if (!node?.name || !node.version || !semver.valid(node.version)) continue
    if (!isDedupableResolution(node.resolution)) continue
    const { peerDepGraphHash = '', patchHash = '' } = parseDepPath(depPath)
    let byPeer = candidates.get(node.name)
    if (byPeer == null) {
      byPeer = new Map()
      candidates.set(node.name, byPeer)
    }
    let byPatch = byPeer.get(peerDepGraphHash)
    if (byPatch == null) {
      byPatch = new Map()
      byPeer.set(peerDepGraphHash, byPatch)
    }
    let bucket = byPatch.get(patchHash)
    if (bucket == null) {
      bucket = []
      byPatch.set(patchHash, bucket)
    }
    bucket.push({ depPath, version: node.version })
  }

  let anyMultiVersionGroup = false
  for (const byPeer of candidates.values()) {
    for (const byPatch of byPeer.values()) {
      for (const [key, bucket] of byPatch) {
        if (bucket.length < 2) {
          byPatch.delete(key)
          continue
        }
        bucket.sort((a, b) => semver.rcompare(a.version, b.version))
        anyMultiVersionGroup = true
      }
    }
  }
  if (!anyMultiVersionGroup) return

  for (const parentDepPath of Object.keys(graph) as DepPath[]) {
    const parent = graph[parentDepPath]
    if (parent.depSpecs == null) continue
    for (const [alias, childDepPath] of Object.entries(parent.children)) {
      const child = graph[childDepPath]
      if (child == null || !child.name || !child.version) continue
      if (!isDedupableResolution(child.resolution)) continue
      const { peerDepGraphHash = '', patchHash = '' } = parseDepPath(childDepPath)
      const bucket = candidates.get(child.name)?.get(peerDepGraphHash)?.get(patchHash)
      if (bucket == null) continue
      const spec = parent.depSpecs[alias]
      if (spec == null || semver.validRange(spec) === null) continue
      const upgrade = findUpgrade(bucket, child.version, spec)
      if (upgrade != null && upgrade !== childDepPath) {
        parent.children[alias] = upgrade
      }
    }
  }
}

function findUpgrade (
  sortedCandidates: Candidate[],
  currentVersion: string,
  spec: string
): DepPath | undefined {
  for (const candidate of sortedCandidates) {
    if (semver.lte(candidate.version, currentVersion)) return undefined
    // Use loose semver semantics, matching the rest of the resolver
    // (see referenceSatisfiesWantedSpec, wantedDepIsLocallyAvailable).
    if (semver.satisfies(candidate.version, spec, { loose: true })) {
      return candidate.depPath
    }
  }
  return undefined
}
