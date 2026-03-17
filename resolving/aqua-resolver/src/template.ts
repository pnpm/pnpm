import type { AquaChecksum, AquaFile, AquaOverride, AquaVersionOverride } from './registry.js'

export interface TargetPlatform {
  os: string
  cpu: string
  goos: string
  goarch: string
}

export interface ExpandedAsset {
  target: { os: string, cpu: string }
  url: string
  assetName: string
  format: string
  files: AquaFile[]
  checksum?: AquaChecksum
}

const DEFAULT_PLATFORMS: TargetPlatform[] = [
  { os: 'darwin', cpu: 'arm64', goos: 'darwin', goarch: 'arm64' },
  { os: 'darwin', cpu: 'x64', goos: 'darwin', goarch: 'amd64' },
  { os: 'linux', cpu: 'x64', goos: 'linux', goarch: 'amd64' },
  { os: 'linux', cpu: 'arm64', goos: 'linux', goarch: 'arm64' },
  { os: 'win32', cpu: 'x64', goos: 'windows', goarch: 'amd64' },
  { os: 'win32', cpu: 'arm64', goos: 'windows', goarch: 'arm64' },
]

export function expandAssets (
  owner: string,
  repo: string,
  version: string,
  override: AquaVersionOverride
): ExpandedAsset[] {
  if (!override.asset) return []

  const platforms = filterSupportedPlatforms(override.supported_envs)
  const assets: ExpandedAsset[] = []

  for (const platform of platforms) {
    const platformOverride = findPlatformOverride(override.overrides, platform)
    const format = resolveFormat(override.format ?? 'tar.gz', platformOverride)
    const replacements = mergeReplacements(override.replacements, platformOverride?.replacements)
    const assetTemplate = platformOverride?.asset ?? override.asset
    const files = platformOverride?.files ?? override.files ?? [{ name: repo }]

    const vars = buildTemplateVars(version, platform, format, replacements)
    const assetName = expandTemplate(assetTemplate, vars)
    const url = `https://github.com/${owner}/${repo}/releases/download/${encodeURIComponent(version)}/${assetName}`

    let checksum: AquaChecksum | undefined
    const checksumConfig = platformOverride?.checksum ?? override.checksum
    if (checksumConfig && 'enabled' in checksumConfig && checksumConfig.enabled === false) {
      checksum = undefined
    } else if (checksumConfig && 'asset' in checksumConfig && checksumConfig.type) {
      checksum = checksumConfig as AquaChecksum
    }

    assets.push({
      target: { os: platform.os, cpu: platform.cpu },
      url,
      assetName,
      format,
      files: files.map((f) => ({
        name: f.name,
        src: f.src ? expandTemplate(f.src, vars) : undefined,
      })),
      checksum,
    })
  }

  return assets
}

function filterSupportedPlatforms (supportedEnvs?: string[]): TargetPlatform[] {
  if (!supportedEnvs) return DEFAULT_PLATFORMS

  return DEFAULT_PLATFORMS.filter((platform) => {
    return supportedEnvs.some((env) => {
      // Format: "os/arch", "os", or "arch"
      const parts = env.split('/')
      if (parts.length === 2) {
        return parts[0] === platform.goos && parts[1] === platform.goarch
      }
      // Single value: could be OS or arch
      return parts[0] === platform.goos || parts[0] === platform.goarch
    })
  })
}

function findPlatformOverride (
  overrides: AquaOverride[] | undefined,
  platform: TargetPlatform
): AquaOverride | undefined {
  if (!overrides) return undefined

  // Find most specific match first (both goos and goarch), then goos-only
  let goosMatch: AquaOverride | undefined
  for (const ov of overrides) {
    const goosMatches = !ov.goos || ov.goos === platform.goos
    const goarchMatches = !ov.goarch || ov.goarch === platform.goarch
    if (goosMatches && goarchMatches) {
      if (ov.goos && ov.goarch) return ov // Exact match
      if (ov.goos) goosMatch = ov
    }
  }
  return goosMatch
}

function resolveFormat (baseFormat: string, platformOverride?: AquaOverride): string {
  return platformOverride?.format ?? baseFormat
}

function mergeReplacements (
  base?: Record<string, string>,
  override?: Record<string, string>
): Record<string, string> {
  if (!base && !override) return {}
  return { ...base, ...override }
}

interface TemplateVars {
  Version: string
  TrimmedVersion: string
  OS: string
  Arch: string
  Format: string
  AssetWithoutExt: string
}

function buildTemplateVars (
  version: string,
  platform: TargetPlatform,
  format: string,
  replacements: Record<string, string>
): TemplateVars {
  const rawOS = platform.goos
  const rawArch = platform.goarch
  const os = replacements[rawOS] ?? rawOS
  const arch = replacements[rawArch] ?? rawArch
  const trimmedVersion = version.startsWith('v') ? version.substring(1) : version

  return {
    Version: version,
    TrimmedVersion: trimmedVersion,
    OS: os,
    Arch: arch,
    Format: format,
    AssetWithoutExt: '', // Will be derived after initial expansion
  }
}

function expandTemplate (template: string, vars: TemplateVars): string {
  let result = template
  result = result.replace(/\{\{\.Version\}\}/g, vars.Version)
  result = result.replace(/\{\{trimV \.Version\}\}/g, vars.TrimmedVersion)
  result = result.replace(/\{\{\.Arch\}\}/g, vars.Arch)
  result = result.replace(/\{\{\.OS\}\}/g, vars.OS)
  result = result.replace(/\{\{\.Format\}\}/g, vars.Format)
  // AssetWithoutExt: expand asset template but remove the format extension
  if (result.includes('{{.AssetWithoutExt}}')) {
    // Build the asset name from the template without the extension part
    const assetFull = expandTemplate(template.replace(/\{\{\.AssetWithoutExt\}\}/g, ''), vars)
    const withoutExt = stripFormatExtension(assetFull, vars.Format)
    result = result.replace(/\{\{\.AssetWithoutExt\}\}/g, withoutExt)
  }
  return result
}

function stripFormatExtension (name: string, format: string): string {
  const ext = `.${format}`
  if (name.endsWith(ext)) {
    return name.substring(0, name.length - ext.length)
  }
  return name
}

export function expandChecksumAssetName (
  checksumTemplate: string,
  assetName: string,
  version: string
): string {
  const trimmedVersion = version.startsWith('v') ? version.substring(1) : version
  let result = checksumTemplate
  result = result.replace(/\{\{\.Asset\}\}/g, assetName)
  result = result.replace(/\{\{\.Version\}\}/g, version)
  result = result.replace(/\{\{trimV \.Version\}\}/g, trimmedVersion)
  return result
}
