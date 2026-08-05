import { spawn } from 'node:child_process'
import fs from 'node:fs/promises'
import path from 'node:path'

import { findRuntimeNodeVersion } from '@pnpm/deps.graph-hasher'
import { engineName } from '@pnpm/engine.runtime.system-version'
import { PnpmError } from '@pnpm/error'
import type { DepPath } from '@pnpm/types'

const PROTOCOL_VERSION = 1

interface ProviderRequestNode {
  name: string
  version: string
  /** Registry tarball resolution. */
  tarball?: string
  integrity?: string
  /** Local directory resolution (file: deps and injected workspace packages) — an install-time snapshot. */
  directory?: string
  /** Git resolution, deterministic by commit. */
  git?: { repo: string, commit: string }
  deps: Record<string, { depPath: string, name: string }>
  engine: string
  /** Patches are deterministic, so their content is just another provider input. */
  patch?: { content: string, hash: string }
  /** The provider may skip an optional package whose build fails instead of aborting. */
  optional?: boolean
}

/**
 * The subset of a dependency graph node the provider integration needs.
 * Both the full-install graph (keyed by depPath, children reference
 * depPaths) and the headless graph (keyed by install dir, children reference
 * dirs) satisfy it: children values are graph keys, and the real depPath is
 * always on the node.
 */
export interface PackageProviderGraphNode {
  depPath: string
  name: string
  version: string
  resolution: unknown
  children: Record<string, string>
  optional?: boolean
  prepare?: boolean
  patch?: { hash: string, patchFilePath?: string }
  dir: string
  modules: string
}

/**
 * Sends the whole dependency graph to the configured external package
 * provider, which materializes every depPath as a read-only directory (e.g.
 * a Nix store path) whose node_modules holds the package next to symlinks to
 * its dependencies. Every graph node's dir and modules are then redirected to
 * the returned location, so the regular direct-dependency, hoist, and bin
 * linking steps work unchanged; importing into the virtual store and running
 * lifecycle scripts must be skipped by the caller (the provider already did
 * both). Optional packages the provider could not build are removed from the
 * graph and returned so the caller can record them as skipped. Any other
 * provider failure aborts the install.
 */
export async function materializeThroughPackageProvider (
  packageProvider: string,
  depGraph: Record<string, PackageProviderGraphNode>,
  opts: {
    lockfileDir: string
  }
): Promise<DepPath[]> {
  const engine = engineName(findRuntimeNodeVersion(Object.values(depGraph).map((node) => node.depPath)))
  const nodes: Record<string, ProviderRequestNode> = {}
  const keyByDepPath = new Map<string, string>()
  for (const [key, node] of Object.entries(depGraph)) {
    keyByDepPath.set(node.depPath, key)
  }
  for (const node of Object.values(depGraph)) {
    const depPath = node.depPath
    const resolution = node.resolution as { type?: string, tarball?: string, integrity?: string, directory?: string, repo?: string, commit?: string }
    let source: Pick<ProviderRequestNode, 'tarball' | 'integrity' | 'directory' | 'git'>
    if (resolution.type == null && resolution.tarball && resolution.integrity) {
      source = { tarball: resolution.tarball, integrity: resolution.integrity }
    } else if (resolution.type === 'directory' && resolution.directory != null) {
      source = { directory: path.resolve(opts.lockfileDir, resolution.directory) }
    } else if (resolution.type === 'git' && resolution.repo && resolution.commit) {
      if ((node as { prepare?: boolean }).prepare === true) {
        throw new PnpmError('PACKAGE_PROVIDER_UNSUPPORTED', `The package provider cannot install ${depPath}: git dependencies that need to be built (prepare) are not supported yet`)
      }
      source = { git: { repo: resolution.repo, commit: resolution.commit } }
    } else {
      throw new PnpmError('PACKAGE_PROVIDER_UNSUPPORTED', `The package provider does not support the resolution of ${depPath} (${resolution.type ?? 'tarball without integrity'})`)
    }
    if (node.patch != null && !node.patch.patchFilePath) {
      throw new PnpmError('PACKAGE_PROVIDER_UNSUPPORTED', `The package provider needs the patch file of ${depPath}, but only its hash is known`)
    }
    const deps: ProviderRequestNode['deps'] = {}
    for (const [alias, childKey] of Object.entries(node.children)) {
      const child = depGraph[childKey]
      if (child == null) continue // skipped package (e.g. an optional dependency for another platform)
      if (alias === node.name) {
        throw new PnpmError('PACKAGE_PROVIDER_UNSUPPORTED', `The package provider cannot install ${depPath}, which depends on a different version of itself`)
      }
      deps[alias] = { depPath: child.depPath, name: child.name }
    }
    nodes[depPath] = {
      name: node.name,
      version: node.version,
      ...source,
      deps,
      engine,
      optional: node.optional === true ? true : undefined,
    }
  }
  if (Object.keys(nodes).length === 0) return []
  await Promise.all(Object.values(depGraph).map(async (node) => {
    if (node.patch?.patchFilePath == null) return
    nodes[node.depPath].patch = {
      content: await fs.readFile(node.patch.patchFilePath, 'utf8'),
      hash: node.patch.hash,
    }
  }))
  const { paths, skipped } = await invokeProvider(packageProvider, {
    protocol: PROTOCOL_VERSION,
    gcRootDir: path.join(opts.lockfileDir, 'node_modules', '.pnpm-nix'),
    nodes,
  })
  const skippedKeys = new Set<string>()
  for (const depPath of skipped) {
    const key = keyByDepPath.get(depPath)
    const node = key == null ? undefined : depGraph[key]
    if (node == null || node.optional !== true) {
      throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider skipped ${depPath}, which is not an optional dependency`)
    }
    skippedKeys.add(key!)
    delete depGraph[key!] // eslint-disable-line @typescript-eslint/no-dynamic-delete
  }
  for (const node of Object.values(depGraph)) {
    if (skippedKeys.size > 0) {
      for (const [alias, childKey] of Object.entries(node.children)) {
        if (skippedKeys.has(childKey)) delete node.children[alias] // eslint-disable-line @typescript-eslint/no-dynamic-delete
      }
    }
    const providedDir = paths[node.depPath]
    if (typeof providedDir !== 'string' || providedDir === '') {
      throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider returned no path for ${node.depPath}`)
    }
    if (!path.isAbsolute(providedDir)) {
      throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider returned a relative path for ${node.depPath}: ${providedDir}`)
    }
    node.modules = path.join(providedDir, 'node_modules')
    node.dir = path.join(node.modules, node.name)
  }
  return skipped as DepPath[]
}

async function invokeProvider (packageProvider: string, request: unknown): Promise<{ paths: Record<string, string>, skipped: string[] }> {
  const stdout = await new Promise<string>((resolve, reject) => {
    // stderr is inherited so provider/Nix build output reaches the user.
    const child = spawn(packageProvider, [], { stdio: ['pipe', 'pipe', 'inherit'] })
    let output = ''
    child.stdout.on('data', (chunk) => {
      output += chunk.toString()
    })
    child.on('error', (err) => {
      reject(new PnpmError('PACKAGE_PROVIDER_FAILED', `Cannot run the package provider at "${packageProvider}": ${err.message}`))
    })
    child.on('close', (code) => {
      if (code === 0) {
        resolve(output)
      } else {
        reject(new PnpmError('PACKAGE_PROVIDER_FAILED', `The package provider at "${packageProvider}" exited with code ${code ?? 'unknown'}`))
      }
    })
    // A provider that exits before reading its whole stdin makes the
    // write fail with EPIPE; the exit code from `close` is the real
    // diagnostic, so writing errors are swallowed here.
    child.stdin.on('error', () => {})
    child.stdin.end(JSON.stringify(request))
  })
  let response: { protocol?: number, paths?: Record<string, string>, skipped?: string[] }
  try {
    response = JSON.parse(stdout)
  } catch {
    throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider at "${packageProvider}" did not return valid JSON`)
  }
  if (response.protocol !== PROTOCOL_VERSION || response.paths == null || typeof response.paths !== 'object' || Array.isArray(response.paths)) {
    throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider at "${packageProvider}" returned an unsupported response (protocol ${String(response.protocol ?? 'missing')})`)
  }
  const skipped = response.skipped ?? []
  if (!Array.isArray(skipped) || skipped.some((depPath) => typeof depPath !== 'string')) {
    throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider at "${packageProvider}" returned a non-string "skipped" list`)
  }
  return { paths: response.paths, skipped }
}
