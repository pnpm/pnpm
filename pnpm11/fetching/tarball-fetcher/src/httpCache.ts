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
  age?: number
  date?: number
}

const CACHE_DIR_NAME = 'v11/tarball-resolutions'

let writeCounter = 0

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
    age: nonNegativeInteger(parsed.age),
    date: typeof parsed.date === 'number' && Number.isFinite(parsed.date) ? parsed.date : undefined,
  }
}

export function storeTarballResolution (cacheDir: string, record: TarballResolutionRecord): void {
  if (!shouldStore(record)) {
    removeTarballResolution(cacheDir, record.url)
    return
  }
  const file = tarballCachePath(cacheDir, record.url)
  const tmp = `${file}.tmp.${process.pid}.${++writeCounter}`
  try {
    fs.mkdirSync(path.dirname(file), { recursive: true })
    fs.writeFileSync(tmp, JSON.stringify({
      url: record.url,
      tarball: record.tarball,
      integrity: record.integrity,
      etag: record.etag ?? null,
      cacheControl: record.cacheControl ?? null,
      fetchedAt: record.fetchedAt,
      age: record.age ?? null,
      date: record.date ?? null,
    }))
    fs.renameSync(tmp, file)
  } catch {
    try {
      fs.rmSync(tmp, { force: true })
    } catch {
      // Non-fatal
    }
  }
}

export function removeTarballResolution (cacheDir: string, url: string): void {
  try {
    fs.rmSync(tarballCachePath(cacheDir, url), { force: true })
  } catch {
    // Non-fatal
  }
}

export type TarballFreshness = 'fresh' | 'revalidate' | 'unusable'

export function tarballFreshness (record: TarballResolutionRecord, now = Date.now()): TarballFreshness {
  const header = record.cacheControl ?? ''
  if (hasDirective(header, 'no-store')) return 'unusable'
  const maxAge = maxAgeSeconds(header)
  if (hasDirective(header, 'no-cache') || maxAge == null) return 'revalidate'
  return currentAgeMs(record, now) < maxAge * 1000 ? 'fresh' : 'revalidate'
}

export function tarballRecordTimestamp (
  headers: { get (name: string): string | null },
  previous?: TarballResolutionRecord,
  now = Date.now()
): Pick<TarballResolutionRecord, 'fetchedAt' | 'age' | 'date'> {
  const age = parseDeltaSeconds(headers.get('age'))
  const date = parseHttpDate(headers.get('date'))
  if (age == null && date == null && previous) {
    return { fetchedAt: now, age: Math.floor(currentAgeMs(previous, now) / 1000), date: undefined }
  }
  return { fetchedAt: now, age, date }
}

function shouldStore (record: TarballResolutionRecord): boolean {
  const header = record.cacheControl ?? ''
  if (hasDirective(header, 'no-store')) return false
  return maxAgeSeconds(header) != null || record.etag != null
}

export function hasDirective (header: string, name: string): boolean {
  return header.split(',').some((part) => part.trim().toLowerCase() === name)
}

function maxAgeSeconds (header: string): number | undefined {
  for (const part of header.split(',')) {
    const [name, value] = part.trim().split('=')
    if (name?.trim().toLowerCase() !== 'max-age' || value == null) continue
    return parseDeltaSeconds(value)
  }
  return undefined
}

function currentAgeMs (record: TarballResolutionRecord, now: number): number {
  const residentMs = Math.max(0, now - record.fetchedAt)
  const ageHeaderMs = record.age != null ? record.age * 1000 : 0
  const apparentMs = record.date != null ? Math.max(0, record.fetchedAt - record.date) : 0
  return Math.max(ageHeaderMs, apparentMs) + residentMs
}

function parseDeltaSeconds (value: string | null | undefined): number | undefined {
  if (value == null) return undefined
  const digits = unquote(value)
  if (!isDecimalDigits(digits)) return undefined
  return nonNegativeInteger(Number(digits))
}

function isDecimalDigits (value: string): boolean {
  if (value.length === 0) return false
  for (const char of value) {
    if (char < '0' || char > '9') return false
  }
  return true
}

function unquote (value: string): string {
  const trimmed = value.trim()
  if (trimmed.length >= 2 && trimmed.startsWith('"') && trimmed.endsWith('"')) {
    return trimmed.slice(1, -1).trim()
  }
  return trimmed
}

function nonNegativeInteger (value: unknown): number | undefined {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : undefined
}

function parseHttpDate (value: string | null): number | undefined {
  if (value == null) return undefined
  const parsed = Date.parse(value)
  return Number.isFinite(parsed) ? parsed : undefined
}

function tarballCachePath (cacheDir: string, url: string): string {
  const digest = createHash('sha256').update(url).digest('hex')
  return path.join(cacheDir, CACHE_DIR_NAME, `${digest}.json`)
}
