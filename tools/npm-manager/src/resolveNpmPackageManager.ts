import {
  type DirectoryResolution,
  type ResolveResult,
  type WantedDependency,
} from '@pnpm/resolver-base'
import { type PkgResolutionId } from '@pnpm/types'
import { resolveNpmVersion } from './resolveNpmVersion.js'

export interface NpmPackageManagerResolveResult extends ResolveResult {
  resolution: DirectoryResolution
  resolvedVia: 'npm-manager'
}

export async function resolveNpmPackageManager (
  ctx: {
    pnpmHomeDir: string
    offline?: boolean
  },
  wantedDependency: WantedDependency
): Promise<NpmPackageManagerResolveResult | null> {
  if (wantedDependency.alias !== 'npm' || !wantedDependency.bareSpecifier?.startsWith('packageManager:')) return null

  const versionSpec = wantedDependency.bareSpecifier.substring('packageManager:'.length)

  const { npmBaseDir, npmVersion } = await resolveNpmVersion(versionSpec, {
    pnpmHomeDir: ctx.pnpmHomeDir,
  })

  return {
    id: `npm@packageManager:${npmVersion}` as PkgResolutionId,
    normalizedBareSpecifier: `packageManager:${versionSpec}`,
    resolvedVia: 'npm-manager',
    manifest: {
      name: 'npm',
      version: npmVersion,
      bin: {
        npm: 'bin/npm-cli.js',
        npx: 'bin/npx-cli.js',
      },
    },
    resolution: {
      type: 'directory',
      directory: npmBaseDir,
    },
  }
}
