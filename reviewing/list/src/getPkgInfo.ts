import { sanitizeInline } from '@pnpm/text.sanitize'
import { type ProjectManifest } from '@pnpm/types'
import path from 'path'
import { readPkg } from './readPkg.js'

interface PkgData {
  alias: string | undefined
  name: string
  version: string
  path: string
  resolved?: string
}

export type PkgInfo = Omit<PkgData, 'name'> & Pick<ProjectManifest, 'description' | 'license' | 'author' | 'homepage'> & {
  from: string
  repository?: string
}

export async function getPkgInfo (pkg: PkgData): Promise<PkgInfo> {
  let manifest
  try {
    manifest = await readPkg(path.join(pkg.path, 'package.json'))
  } catch {
    // This will probably never happen
    manifest = {
      description: '[Could not find additional info about this dependency]',
    }
  }
  return {
    alias: pkg.alias,
    from: pkg.name,

    version: pkg.version,

    resolved: pkg.resolved,

    description: manifest.description,
    license: manifest.license,
    author: manifest.author,

    homepage: manifest.homepage,
    repository: (manifest.repository && (
      typeof manifest.repository === 'string' ? manifest.repository : manifest.repository.url
    )) ?? undefined,
    path: pkg.path,
  }
}

/**
 * The `--long` label lines taken from the package's own manifest. The
 * manifest is authored by the package publisher, so each line is stripped
 * of terminal control characters.
 */
export function renderLongInfoLines (pkg: PkgInfo): string[] {
  return [pkg.description, pkg.repository, pkg.homepage]
    .filter((line): line is string => typeof line === 'string' && line !== '')
    .map(sanitizeInline)
}
