import { execFileSync } from 'node:child_process'
import { release as osRelease } from 'node:os'

import {
  linuxGlibcCompatibilityTag,
  linuxGlibcSupportedTags,
  macOSCompatibilityTag,
  macOSSupportedTags,
  windowsCompatibilityTag,
  windowsSupportedTags,
} from './compatibilityTags.js'
import type { LinuxGlibcPlatform, MacOSPlatform, WindowsPlatform } from './sharedArtifactTypes.js'

export type ArtifactPlatform =
  | { kind: 'linuxGlibc', platform: LinuxGlibcPlatform }
  | { kind: 'macOS', platform: MacOSPlatform }
  | { kind: 'windows', platform: WindowsPlatform }

export function currentArtifactPlatform (nodeVersion?: string): ArtifactPlatform | undefined {
  if (!['x64', 'arm64'].includes(process.arch)) return undefined
  const nodeMajor = parseNodeMajor(nodeVersion ?? process.version)
  if (nodeMajor == null) return undefined
  switch (process.platform) {
    case 'linux': return linuxGlibcArtifactPlatform(nodeMajor)
    case 'darwin': return macOSArtifactPlatform(nodeMajor)
    case 'win32': return windowsArtifactPlatform(nodeMajor)
    default: return undefined
  }
}

function parseNodeMajor (version: string): number | undefined {
  const nodeMajor = Number((version.startsWith('v') ? version.slice(1) : version).split('.')[0])
  if (!Number.isSafeInteger(nodeMajor) || nodeMajor <= 0) return undefined
  return nodeMajor
}

function linuxGlibcArtifactPlatform (nodeMajor: number): ArtifactPlatform | undefined {
  const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } }
  const [glibcMajor, glibcMinor] = report.header?.glibcVersionRuntime?.split('.').map(Number) ?? []
  if (![glibcMajor, glibcMinor].every(Number.isSafeInteger)) return undefined
  return {
    kind: 'linuxGlibc',
    platform: { architecture: process.arch, nodeMajor, glibcMajor, glibcMinor },
  }
}

function macOSArtifactPlatform (nodeMajor: number): ArtifactPlatform | undefined {
  const version = macOSProductVersion()
  if (version == null) return undefined
  return {
    kind: 'macOS',
    platform: {
      architecture: process.arch,
      nodeMajor,
      macOSMajor: version.major,
      macOSMinor: version.minor,
    },
  }
}

function windowsArtifactPlatform (nodeMajor: number): ArtifactPlatform | undefined {
  const version = windowsKernelVersion(osRelease())
  if (version == null) return undefined
  return {
    kind: 'windows',
    platform: {
      architecture: process.arch,
      nodeMajor,
      windowsMajor: version.major,
      windowsMinor: version.minor,
      windowsBuild: version.build,
    },
  }
}

export function artifactCompatibilityTag (artifactPlatform: ArtifactPlatform): string {
  switch (artifactPlatform.kind) {
    case 'linuxGlibc': return linuxGlibcCompatibilityTag(artifactPlatform.platform)
    case 'macOS': return macOSCompatibilityTag(artifactPlatform.platform)
    case 'windows': return windowsCompatibilityTag(artifactPlatform.platform)
  }
}

export function artifactSupportedTags (artifactPlatform: ArtifactPlatform): string[] {
  switch (artifactPlatform.kind) {
    case 'linuxGlibc': return linuxGlibcSupportedTags(artifactPlatform.platform)
    case 'macOS': return macOSSupportedTags(artifactPlatform.platform)
    case 'windows': return windowsSupportedTags(artifactPlatform.platform)
  }
}

let cachedMacOSProductVersion: { major: number, minor: number } | null | undefined

function macOSProductVersion (): { major: number, minor: number } | undefined {
  if (cachedMacOSProductVersion !== undefined) return cachedMacOSProductVersion ?? undefined
  try {
    const [major, minor] = execFileSync('/usr/bin/sw_vers', ['-productVersion'], {
      encoding: 'utf8',
      timeout: 5_000,
    }).trim().split('.').map(Number)
    cachedMacOSProductVersion =
      isIntegerInRange(major, 1, 1_000_000) && isIntegerInRange(minor, 0, 1_000_000)
        ? { major, minor }
        : null
  } catch {
    cachedMacOSProductVersion = null
  }
  return cachedMacOSProductVersion ?? undefined
}

function windowsKernelVersion (release: string): { major: number, minor: number, build: number } | undefined {
  const components = release.split('.')
  if (components.length !== 3) return undefined
  const [major, minor, build] = components.map(Number)
  if (
    !isIntegerInRange(major, 1, 1_000) ||
    !isIntegerInRange(minor, 0, 1_000) ||
    !isIntegerInRange(build, 1, 1_000_000)
  ) return undefined
  return { major, minor, build }
}

function isIntegerInRange (value: number, min: number, exclusiveMax: number): boolean {
  return Number.isSafeInteger(value) && value >= min && value < exclusiveMax
}
