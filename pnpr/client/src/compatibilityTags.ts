import { createHash } from 'node:crypto'

import { validateScalar } from './artifactEncoding.js'
import {
  COMPATIBILITY_TAG_SCHEMA,
  type CompatibilityConstraints,
  type LinuxGlibcPlatform,
  type MacOSPlatform,
  type WindowsPlatform,
} from './sharedArtifactTypes.js'

export function validateCompatibility (compatibility: CompatibilityConstraints): void {
  if (compatibility?.kind === 'universal') return
  if (compatibility?.kind !== 'tagged' || !Array.isArray(compatibility.tags)) {
    throw new Error('Shared artifact compatibility has an unknown kind')
  }
  if (compatibility.tags.length === 0 || compatibility.tags.length > 64) {
    throw new Error('Tagged compatibility must contain between 1 and 64 tags')
  }
  const unique = new Set<string>()
  for (const tag of compatibility.tags) {
    validateCompatibilityTag(tag)
    if (unique.has(tag)) throw new Error(`Duplicate compatibility tag ${JSON.stringify(tag)}`)
    unique.add(tag)
  }
}

export function compatibilityRank (constraints: CompatibilityConstraints, supportedTags: string[]): number | undefined {
  try {
    validateSupportedTags(supportedTags)
  } catch {
    return undefined
  }
  return rankCompatibility(constraints, supportedTags)
}

export function rankCompatibility (constraints: CompatibilityConstraints, supportedTags: string[]): number | undefined {
  if (constraints.kind === 'universal') return Number.MAX_SAFE_INTEGER
  try {
    return bestTaggedRank(constraints.tags, supportedTags)
  } catch {
    return undefined
  }
}

/**
 * @throws if a tag that is not an exact match cannot be parsed.
 */
function bestTaggedRank (artifactTags: string[], supportedTags: string[]): number | undefined {
  let bestRank: number | undefined
  for (let index = 0; index < supportedTags.length; index++) {
    for (const artifactTag of artifactTags) {
      const rank = rankTagPair(supportedTags[index], artifactTag, index)
      if (rank != null) bestRank = Math.min(bestRank ?? rank, rank)
    }
  }
  return bestRank
}

function rankTagPair (supportedTag: string, artifactTag: string, supportedTagIndex: number): number | undefined {
  if (artifactTag === supportedTag) return supportedTagIndex
  const consumer = parseVersionedCompatibilityTag(supportedTag)
  const artifact = parseVersionedCompatibilityTag(artifactTag)
  if (consumer == null || artifact == null || !isSamePlatformLine(consumer, artifact)) return undefined
  const consumerVersion = versionedPlatformRank(consumer)
  const artifactVersion = versionedPlatformRank(artifact)
  if (consumerVersion < artifactVersion) return undefined
  return 64 + supportedTagIndex * 1_000_000_000_000 + consumerVersion - artifactVersion
}

function isSamePlatformLine (consumer: VersionedPlatform, artifact: VersionedPlatform): boolean {
  return consumer.kind === artifact.kind &&
    consumer.platform.architecture === artifact.platform.architecture &&
    consumer.platform.nodeMajor === artifact.platform.nodeMajor
}

export function linuxGlibcCompatibilityTag (
  platform: LinuxGlibcPlatform
): string {
  const { architecture, nodeMajor, glibcMajor, glibcMinor } = platform
  const tag = `${COMPATIBILITY_TAG_SCHEMA}:linux-${architecture}-node${nodeMajor}-glibc${glibcMajor}.${glibcMinor}`
  validateCompatibilityTag(tag)
  return tag
}

export function linuxGlibcSupportedTags (
  platform: LinuxGlibcPlatform
): string[] {
  const { architecture, nodeMajor, glibcMajor, glibcMinor } = platform
  if (!Number.isSafeInteger(glibcMinor) || glibcMinor < 0 || glibcMinor >= 64) {
    throw new Error('Shared artifact glibc floor expansion exceeds 64 tags')
  }
  return Array.from(
    { length: glibcMinor + 1 },
    (_, index) => linuxGlibcCompatibilityTag({
      architecture,
      nodeMajor,
      glibcMajor,
      glibcMinor: glibcMinor - index,
    })
  )
}

export function macOSSupportedTags (platform: MacOSPlatform): string[] {
  return [macOSCompatibilityTag(platform)]
}

export function macOSCompatibilityTag (
  platform: MacOSPlatform
): string {
  const { architecture, nodeMajor, macOSMajor, macOSMinor } = platform
  const tag = `${COMPATIBILITY_TAG_SCHEMA}:darwin-${architecture}-node${nodeMajor}-macos${macOSMajor}.${macOSMinor}`
  validateCompatibilityTag(tag)
  return tag
}

export function windowsSupportedTags (platform: WindowsPlatform): string[] {
  return [windowsCompatibilityTag(platform)]
}

export function windowsCompatibilityTag (
  platform: WindowsPlatform
): string {
  const { architecture, nodeMajor, windowsMajor, windowsMinor, windowsBuild } = platform
  const tag = `${COMPATIBILITY_TAG_SCHEMA}:win32-${architecture}-node${nodeMajor}-windows${windowsMajor}.${windowsMinor}.${windowsBuild}`
  validateCompatibilityTag(tag)
  return tag
}

export function platformFingerprint (supportedTags: string[]): string {
  validateSupportedTags(supportedTags)
  const hash = createHash('sha256').update('pnpm-platform-fingerprint-v1\0')
  for (const tag of supportedTags) hash.update(tag).update('\0')
  return hash.digest('hex')
}

export function validateSupportedTags (tags: string[]): void {
  if (!Array.isArray(tags) || tags.length > 64) {
    throw new Error('Shared artifact consumer advertises more than 64 supported tags')
  }
  const unique = new Set<string>()
  for (const tag of tags) {
    validateCompatibilityTag(tag)
    if (unique.has(tag)) throw new Error(`Duplicate consumer compatibility tag ${JSON.stringify(tag)}`)
    unique.add(tag)
  }
}

const RUNTIME_VALIDATORS = new Map<string, (runtime: string) => void>([
  ['linux', validateGlibcRuntime],
  ['darwin', validateMacOSRuntime],
  ['win32', validateWindowsRuntime],
])

export function validateCompatibilityTag (tag: string): void {
  validateScalar('compatibility tag', tag, 512)
  if (!tag.startsWith(`${COMPATIBILITY_TAG_SCHEMA}:`)) {
    throw new Error('Shared artifact compatibility tag uses an unknown schema')
  }
  const parts = tag.slice(COMPATIBILITY_TAG_SCHEMA.length + 1).split('-')
  if (parts.length !== 4) throw new Error('Shared artifact compatibility tag has the wrong number of dimensions')
  const [os, architecture, node, runtime] = parts
  if (!['x64', 'arm64'].includes(architecture)) {
    throw new Error('Shared artifact compatibility tag only supports x64 and arm64 in v1')
  }
  parseCanonicalNumber(node.startsWith('node') ? node.slice(4) : '', 'Node major version', false)
  const validateRuntime = RUNTIME_VALIDATORS.get(os)
  if (validateRuntime == null) {
    throw new Error('Shared artifact compatibility tag only supports Linux, macOS, and Windows in v1')
  }
  validateRuntime(runtime)
}

function validateGlibcRuntime (runtime: string): void {
  const glibc = runtime.startsWith('glibc') ? runtime.slice(5) : ''
  const version = glibc.split('.')
  if (version.length !== 2) throw new Error('Shared artifact glibc floor must be major.minor')
  parseCanonicalNumber(version[0], 'glibc major version', false)
  parseCanonicalNumber(version[1], 'glibc minor version', true)
}

function validateMacOSRuntime (runtime: string): void {
  const macOS = runtime.startsWith('macos') ? runtime.slice(5) : ''
  const version = macOS.split('.')
  if (version.length !== 2) throw new Error('Shared artifact macOS floor must be major.minor')
  const major = parseCanonicalNumber(version[0], 'macOS major version', false)
  const minor = parseCanonicalNumber(version[1], 'macOS minor version', true)
  if (major >= 1_000_000 || minor >= 1_000_000) {
    throw new Error('Shared artifact macOS version component is too large')
  }
}

function validateWindowsRuntime (runtime: string): void {
  const windows = runtime.startsWith('windows') ? runtime.slice(7) : ''
  const version = windows.split('.')
  if (version.length !== 3) throw new Error('Shared artifact Windows floor must be major.minor.build')
  const major = parseCanonicalNumber(version[0], 'Windows major version', false)
  const minor = parseCanonicalNumber(version[1], 'Windows minor version', true)
  const build = parseCanonicalNumber(version[2], 'Windows build number', false)
  if (major >= 1_000 || minor >= 1_000 || build >= 1_000_000) {
    throw new Error('Shared artifact Windows version component is too large')
  }
}

type VersionedPlatform =
  | { kind: 'macOS', platform: MacOSPlatform }
  | { kind: 'windows', platform: WindowsPlatform }

function parseVersionedCompatibilityTag (tag: string): VersionedPlatform | undefined {
  validateCompatibilityTag(tag)
  const [os, architecture, node, runtime] = tag.slice(COMPATIBILITY_TAG_SCHEMA.length + 1).split('-')
  const nodeMajor = Number(node.slice(4))
  if (os === 'darwin') {
    const [macOSMajor, macOSMinor] = runtime.slice(5).split('.').map(Number)
    return {
      kind: 'macOS',
      platform: { architecture, nodeMajor, macOSMajor, macOSMinor },
    }
  }
  if (os === 'win32') {
    const [windowsMajor, windowsMinor, windowsBuild] = runtime.slice(7).split('.').map(Number)
    return {
      kind: 'windows',
      platform: { architecture, nodeMajor, windowsMajor, windowsMinor, windowsBuild },
    }
  }
  return undefined
}

function versionedPlatformRank (versionedPlatform: VersionedPlatform): number {
  if (versionedPlatform.kind === 'macOS') {
    return versionedPlatform.platform.macOSMajor * 1_000_000 + versionedPlatform.platform.macOSMinor
  }
  return (
    versionedPlatform.platform.windowsMajor * 1_000_000_000 +
    versionedPlatform.platform.windowsMinor * 1_000_000 +
    versionedPlatform.platform.windowsBuild
  )
}

function parseCanonicalNumber (value: string, label: string, allowZero: boolean): number {
  if (value.length === 0 || Array.from(value).some(character => character < '0' || character > '9')) {
    throw new Error(`Shared artifact compatibility tag has an invalid ${label}`)
  }
  const number = Number(value)
  if (!Number.isSafeInteger(number) || String(number) !== value || (!allowZero && number === 0)) {
    throw new Error(`Shared artifact compatibility tag has a non-canonical ${label}`)
  }
  return number
}
