import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

export interface TarballResolutionRecord {
  url: string
  tarball: string
  integrity: string
  etag?: string
  cacheControl?: string
  fetchedAt: number
}

const CACHE_DIR_NAME = 'v11/tarball-resolutions'

export function loadTarballResolution (cacheDir: string, url: string): TarballResolutionRecord | undefined {
  let text: string
  try {
    text = fs.readFileSync(tarballCachePath(cacheDir, url), 'utf8')
  } catch {
    return undefined // missing cache entry
  }
  let parsed: Partial<TarballResolutionRecord>
  try {
    parsed = JSON.parse(text) as Partial<TarballResolutionRecord>
  } catch {
    return undefined // unreadable cache entry
  }
  if (parsed.url !== url || typeof parsed.tarball !== 'string' || typeof parsed.integrity !== 'string') {
    return undefined
  }
  return {
    url: parsed.url,
    tarball: parsed.tarball,
    integrity: parsed.integrity,
    etag: typeof parsed.etag === 'string' ? parsed.etag : undefined,
    cacheControl: typeof parsed.cacheControl === 'string' ? parsed.cacheControl : undefined,
    fetchedAt: typeof parsed.fetchedAt === 'number' ? parsed.fetchedAt : 0,
  }
}

export function storeTarballResolution (cacheDir: string, record: TarballResolutionRecord): void {
  if (!shouldStore(record)) {
    removeTarballResolution(cacheDir, record.url)
    return
  }
  const file = tarballCachePath(cacheDir, record.url)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  const tmp = `${file}.tmp`
  fs.writeFileSync(tmp, JSON.stringify({
    url: record.url,
    tarball: record.tarball,
    integrity: record.integrity,
    etag: record.etag ?? null,
    cacheControl: record.cacheControl ?? null,
    fetchedAt: record.fetchedAt,
  }))
  fs.renameSync(tmp, file)
}

export function removeTarballResolution (cacheDir: string, url: string): void {
  fs.rmSync(tarballCachePath(cacheDir, url), { force: true })
}

export type TarballFreshness = 'fresh' | 'revalidate' | 'unusable'

export function tarballFreshness (record: TarballResolutionRecord, now = Date.now()): TarballFreshness {
  const header = record.cacheControl ?? ''
  if (hasDirective(header, 'no-store')) return 'unusable'
  const maxAge = maxAgeSeconds(header)
  if (hasDirective(header, 'no-cache') || maxAge == null) return 'revalidate'
  return now - record.fetchedAt < maxAge * 1000 ? 'fresh' : 'revalidate'
}

function shouldStore (record: TarballResolutionRecord): boolean {
  const header = record.cacheControl ?? ''
  if (hasDirective(header, 'no-store')) return false
  return maxAgeSeconds(header) != null || record.etag != null
}

function hasDirective (header: string, name: string): boolean {
  return header.split(',').some((part) => part.trim().toLowerCase() === name)
}

function maxAgeSeconds (header: string): number | undefined {
  for (const part of header.split(',')) {
    const [name, value] = part.trim().split('=')
    if (name?.trim().toLowerCase() !== 'max-age' || value == null) continue
    const parsed = Number(value.trim().replace(/"/g, ''))
    if (Number.isFinite(parsed)) return parsed
  }
  return undefined
}

function tarballCachePath (cacheDir: string, url: string): string {
  const digest = createHash('sha256').update(url).digest('hex')
  return path.join(cacheDir, CACHE_DIR_NAME, `${digest}.json`)
}
