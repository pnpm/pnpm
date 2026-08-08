import { createHash } from 'node:crypto'
import { createWriteStream } from 'node:fs'
import { pipeline } from 'node:stream/promises'

import type { Packument } from './resolveVersion.js'

interface VersionMeta {
  dist: {
    tarball: string
    integrity?: string
    shasum?: string
  }
}

const ABBREVIATED_PACKUMENT = 'application/vnd.npm.install-v1+json'

export const DEFAULT_REGISTRY = 'https://registry.npmjs.org/'

/** The registry npx/npm is configured with, so a mirror stays a mirror. */
export function registryFromEnv (): string {
  const registry = process.env.npm_config_registry ?? process.env.NPM_CONFIG_REGISTRY ?? DEFAULT_REGISTRY
  return registry.endsWith('/') ? registry : `${registry}/`
}

export async function fetchPackument (registry: string, pkgName: string): Promise<Packument> {
  return fetchJson<Packument>(new URL(pkgName, registry), ABBREVIATED_PACKUMENT)
}

export async function fetchVersionMeta (registry: string, pkgName: string, version: string): Promise<VersionMeta> {
  return fetchJson<VersionMeta>(new URL(`${pkgName}/${version}`, registry), 'application/json')
}

/**
 * Streams `meta.dist.tarball` to `dest`, verifying the checksum the registry
 * published for it. A mismatch removes nothing — the caller discards the whole
 * temporary directory.
 */
export async function downloadTarball (meta: VersionMeta, dest: string): Promise<void> {
  const response = await request(new URL(meta.dist.tarball))
  const [algorithm, expected] = checksum(meta)
  const hash = createHash(algorithm)
  const body = response.body as unknown as AsyncIterable<Uint8Array>
  await pipeline(
    async function * () {
      for await (const chunk of body) {
        hash.update(chunk)
        yield chunk
      }
    },
    createWriteStream(dest)
  )
  const actual = hash.digest(algorithm === 'sha1' ? 'hex' : 'base64')
  if (actual !== expected) {
    throw new Error(`Checksum mismatch for ${meta.dist.tarball}. Expected ${algorithm} ${expected} but got ${actual}.`)
  }
}

function checksum (meta: VersionMeta): [algorithm: string, expected: string] {
  if (meta.dist.integrity) {
    const [algorithm, expected] = meta.dist.integrity.split('-')
    return [algorithm, expected]
  }
  if (meta.dist.shasum) {
    return ['sha1', meta.dist.shasum]
  }
  throw new Error(`The registry published no checksum for ${meta.dist.tarball}, so it cannot be verified.`)
}

async function fetchJson<T> (url: URL, accept: string): Promise<T> {
  const response = await request(url, accept)
  return await response.json() as T
}

async function request (url: URL, accept?: string): Promise<Response> {
  let response: Response
  try {
    response = await fetch(url, accept ? { headers: { accept } } : undefined)
  } catch (err) {
    throw new Error(`Could not reach ${url.href}: ${(err as Error).message}`, { cause: err })
  }
  if (!response.ok) {
    throw new Error(`Could not download ${url.href}: ${response.status} ${response.statusText}`)
  }
  if (response.body == null) {
    throw new Error(`Empty response from ${url.href}`)
  }
  return response
}
