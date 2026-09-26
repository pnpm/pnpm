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
  const parsed = parseWantedDependency(spec)
  if (parsed.bareSpecifier) {
    const bare = parsed.bareSpecifier
    if (bare.startsWith('npm:')) {
      const aliasedName = bare.slice(4)
      const pkgName = aliasedName.startsWith('@')
        ? (aliasedName.indexOf('@', 1) !== -1 ? aliasedName.slice(0, aliasedName.indexOf('@', 1)) : aliasedName)
        : aliasedName.split('@')[0]
      return `npm:${pkgName}`
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
  }
  return `npm:${alias}`
}
