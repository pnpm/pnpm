import _readProjectManifest, * as utils from '@pnpm/read-project-manifest'
import { ProjectManifest } from '@pnpm/types'
import { packageIsInstallable } from './packageIsInstallable'

export async function readProjectManifest (
  importerDir: string,
  opts: { engineStrict?: boolean },
): Promise<{
  fileName: string,
  manifest: ProjectManifest,
  writeProjectManifest: (manifest: ProjectManifest, force?: boolean) => Promise<void>,
}> {
  const { fileName, manifest, writeProjectManifest } = await _readProjectManifest(importerDir)
  packageIsInstallable(importerDir, manifest as any, opts) // tslint:disable-line:no-any
  return { fileName, manifest, writeProjectManifest }
}

export async function readProjectManifestOnly (
  importerDir: string,
  opts: { engineStrict?: boolean },
): Promise<ProjectManifest> {
  const manifest = await utils.readProjectManifestOnly(importerDir)
  packageIsInstallable(importerDir, manifest as any, opts) // tslint:disable-line:no-any
  return manifest
}

export async function tryReadProjectManifest (
  importerDir: string,
  opts: { engineStrict?: boolean },
): Promise<{
  fileName: string,
  manifest: ProjectManifest | null,
  writeProjectManifest: (manifest: ProjectManifest, force?: boolean) => Promise<void>,
}> {
  const { fileName, manifest, writeProjectManifest } = await utils.tryReadProjectManifest(importerDir)
  if (!manifest) return { fileName, manifest, writeProjectManifest }
  packageIsInstallable(importerDir, manifest as any, opts) // tslint:disable-line:no-any
  return { fileName, manifest, writeProjectManifest }
}
