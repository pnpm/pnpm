import _readImporterManifest, * as utils from '@pnpm/read-importer-manifest'
import { ImporterManifest } from '@pnpm/types'
import { packageIsInstallable } from './packageIsInstallable'

export async function readImporterManifest (
  importerDir: string,
  opts: { engineStrict?: boolean },
): Promise<{
  fileName: string,
  manifest: ImporterManifest,
  writeImporterManifest: (manifest: ImporterManifest, force?: boolean) => Promise<void>,
}> {
  const { fileName, manifest, writeImporterManifest } = await _readImporterManifest(importerDir)
  packageIsInstallable(importerDir, manifest as any, opts) // tslint:disable-line:no-any
  return { fileName, manifest, writeImporterManifest }
}

export async function readImporterManifestOnly (
  importerDir: string,
  opts: { engineStrict?: boolean },
): Promise<ImporterManifest> {
  const manifest = await utils.readImporterManifestOnly(importerDir)
  packageIsInstallable(importerDir, manifest as any, opts) // tslint:disable-line:no-any
  return manifest
}

export async function tryReadImporterManifest (
  importerDir: string,
  opts: { engineStrict?: boolean },
): Promise<{
  fileName: string,
  manifest: ImporterManifest | null,
  writeImporterManifest: (manifest: ImporterManifest, force?: boolean) => Promise<void>,
}> {
  const { fileName, manifest, writeImporterManifest } = await utils.tryReadImporterManifest(importerDir)
  if (!manifest) return { fileName, manifest, writeImporterManifest }
  packageIsInstallable(importerDir, manifest as any, opts) // tslint:disable-line:no-any
  return { fileName, manifest, writeImporterManifest }
}
