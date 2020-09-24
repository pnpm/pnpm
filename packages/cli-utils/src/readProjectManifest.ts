import _readProjectManifest, * as utils from '@pnpm/read-project-manifest'
import { ProjectManifest } from '@pnpm/types'
import { packageIsInstallable } from './packageIsInstallable'

export async function readProjectManifest (
  projectDir: string,
  opts: { engineStrict?: boolean }
): Promise<{
    fileName: string
    manifest: ProjectManifest
    writeProjectManifest: (manifest: ProjectManifest, force?: boolean) => Promise<void>
  }> {
  const { fileName, manifest, writeProjectManifest } = await _readProjectManifest(projectDir)
  packageIsInstallable(projectDir, manifest as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
  return { fileName, manifest, writeProjectManifest }
}

export async function readProjectManifestOnly (
  projectDir: string,
  opts: { engineStrict?: boolean }
): Promise<ProjectManifest> {
  const manifest = await utils.readProjectManifestOnly(projectDir)
  packageIsInstallable(projectDir, manifest as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
  return manifest
}

export async function tryReadProjectManifest (
  projectDir: string,
  opts: { engineStrict?: boolean }
): Promise<{
    fileName: string
    manifest: ProjectManifest | null
    writeProjectManifest: (manifest: ProjectManifest, force?: boolean) => Promise<void>
  }> {
  const { fileName, manifest, writeProjectManifest } = await utils.tryReadProjectManifest(projectDir)
  if (!manifest) return { fileName, manifest, writeProjectManifest }
  packageIsInstallable(projectDir, manifest as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
  return { fileName, manifest, writeProjectManifest }
}
