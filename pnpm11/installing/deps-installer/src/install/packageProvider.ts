import { spawn } from 'node:child_process'
import path from 'node:path'

import { findRuntimeNodeVersion } from '@pnpm/deps.graph-hasher'
import { engineName } from '@pnpm/engine.runtime.system-version'
import { PnpmError } from '@pnpm/error'
import type { DependenciesGraph } from '@pnpm/installing.deps-resolver'
import type { TarballResolution } from '@pnpm/store.controller-types'
import type { DepPath } from '@pnpm/types'

const PROTOCOL_VERSION = 1

interface ProviderRequestNode {
  name: string
  version: string
  tarball: string
  integrity: string
  deps: Record<string, { depPath: string, name: string }>
  engine: string
}

/**
 * Sends the whole dependency graph to the configured external package
 * provider, which materializes every depPath as a read-only directory (e.g.
 * a Nix store path) whose node_modules holds the package next to symlinks to
 * its dependencies. Every graph node's dir and modules are then repointed at
 * the returned location, so the regular direct-dependency, hoist, and bin
 * linking steps work unchanged; importing into the virtual store and running
 * lifecycle scripts must be skipped by the caller (the provider already did
 * both). Any provider failure aborts the install.
 */
export async function materializeThroughPackageProvider (
  packageProvider: string,
  depGraph: DependenciesGraph,
  opts: {
    lockfileDir: string
  }
): Promise<void> {
  const engine = engineName(findRuntimeNodeVersion(Object.keys(depGraph)))
  const nodes: Record<string, ProviderRequestNode> = {}
  for (const [depPath, node] of Object.entries(depGraph)) {
    const resolution = node.resolution as TarballResolution & { type?: string }
    if (resolution.type != null || !resolution.tarball || !resolution.integrity) {
      throw new PnpmError('PACKAGE_PROVIDER_UNSUPPORTED', `The package provider only supports registry tarball dependencies, but ${depPath} does not resolve to a tarball with integrity`)
    }
    if (node.patch != null) {
      throw new PnpmError('PACKAGE_PROVIDER_UNSUPPORTED', `The package provider does not support patched dependencies (${depPath})`)
    }
    const deps: ProviderRequestNode['deps'] = {}
    for (const [alias, childDepPath] of Object.entries(node.children as Record<string, DepPath>)) {
      const child = depGraph[childDepPath]
      if (child == null) continue // skipped package (e.g. an optional dependency for another platform)
      if (alias === node.name) {
        throw new PnpmError('PACKAGE_PROVIDER_UNSUPPORTED', `The package provider cannot install ${depPath}, which depends on a different version of itself`)
      }
      deps[alias] = { depPath: childDepPath, name: child.name }
    }
    nodes[depPath] = {
      name: node.name,
      version: node.version,
      tarball: resolution.tarball,
      integrity: resolution.integrity,
      deps,
      engine,
    }
  }
  if (Object.keys(nodes).length === 0) return
  const paths = await invokeProvider(packageProvider, {
    protocol: PROTOCOL_VERSION,
    gcRootDir: path.join(opts.lockfileDir, 'node_modules', '.pnpm-nix'),
    nodes,
  })
  for (const [depPath, node] of Object.entries(depGraph)) {
    const providedDir = paths[depPath]
    if (typeof providedDir !== 'string') {
      throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider returned no path for ${depPath}`)
    }
    node.modules = path.join(providedDir, 'node_modules')
    node.dir = path.join(node.modules, node.name)
  }
}

async function invokeProvider (packageProvider: string, request: unknown): Promise<Record<string, string>> {
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
    child.stdin.end(JSON.stringify(request))
  })
  let response: { protocol?: number, paths?: Record<string, string> }
  try {
    response = JSON.parse(stdout)
  } catch {
    throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider at "${packageProvider}" did not return valid JSON`)
  }
  if (response.protocol !== PROTOCOL_VERSION || response.paths == null) {
    throw new PnpmError('PACKAGE_PROVIDER_RESULT_INVALID', `The package provider at "${packageProvider}" returned an unsupported response (protocol ${String(response.protocol ?? 'missing')})`)
  }
  return response.paths
}
