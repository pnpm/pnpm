import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import HostedGit from 'hosted-git-info'

export function isSameSource (spec1: string, spec2: string, alias: string): boolean {
  if (spec1 === spec2) {
    return true
  }
  return getSpecifierSource(spec1, alias) === getSpecifierSource(spec2, alias)
}

function getSpecifierSource (spec: string, alias: string): string {
  const hosted = HostedGit.fromUrl(spec)
  if (hosted) {
    return `git:${hosted.type}:${hosted.user}:${hosted.project}`
  }
  if (spec.startsWith('git+') || spec.startsWith('git:') || spec.endsWith('.git')) {
    const repoPart = spec.split('#')[0]
    return `git:${repoPart}`
  }
  const { bareSpecifier } = parseWantedDependency(spec)
  return (bareSpecifier ? getBareSpecifierSource(bareSpecifier, alias) : undefined) ?? `npm:${alias}`
}

function getBareSpecifierSource (bare: string, alias: string): string | undefined {
  if (bare.startsWith('npm:')) {
    return `npm:${getAliasedPackageName(bare.slice(4))}`
  }
  if (bare.startsWith('file:') || bare.startsWith('link:') || bare.startsWith('portal:')) {
    return `file:${bare}`
  }
  if (bare.startsWith('workspace:')) {
    return `workspace:${alias}`
  }
  if (bare.startsWith('catalog:')) {
    return `catalog:${bare}`
  }
  if (bare.startsWith('http:') || bare.startsWith('https:')) {
    const urlPart = bare.split('#')[0]
    return `url:${urlPart}`
  }
  return undefined
}

function getAliasedPackageName (aliasedName: string): string {
  if (!aliasedName.startsWith('@')) return aliasedName.split('@')[0]
  const versionSeparatorIndex = aliasedName.indexOf('@', 1)
  return versionSeparatorIndex !== -1 ? aliasedName.slice(0, versionSeparatorIndex) : aliasedName
}
