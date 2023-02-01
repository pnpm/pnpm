import { ProjectManifest } from '@pnpm/types'
import path from 'path'
import { readPkg } from './readPkg'

interface PkgData {
  alias: string | undefined
  name: string
  version: string
  path: string
  resolved?: string
}

export type PkgInfo = Omit<PkgData, 'name' | 'path'> & Pick<ProjectManifest, 'description' | 'license' | 'author' | 'homepage'> & {
  from: string
  repository?: string
}

export async function getPkgInfo (pkg: PkgData): Promise<PkgInfo> {
  let manifest
  try {
    manifest = await readPkg(path.join(pkg.path, 'package.json'))
  } catch (err: any) { // eslint-disable-line
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
  }
}
