import packageIsInstallable from '@pnpm/package-is-installable'
import _readImporterManifest, * as utils from '@pnpm/read-importer-manifest'
import { ImporterManifest } from '@pnpm/types'
import packageManager from './pnpmPkgJson'

export default async function readImporterManifest (importerDir: string): Promise<{
  fileName: string,
  manifest: ImporterManifest,
  writeImporterManifest: (manifest: ImporterManifest, force?: boolean) => Promise<void>,
}> {
  const { fileName, manifest, writeImporterManifest } = await _readImporterManifest(importerDir)
  packageIsInstallable(importerDir, manifest as any, { // tslint:disable-line:no-any
    engineStrict: true,
    optional: false,
    pnpmVersion: packageManager.stableVersion,
    prefix: importerDir,
  })
  return { fileName, manifest, writeImporterManifest }
}

export async function readImporterManifestOnly (importerDir: string): Promise<ImporterManifest> {
  const manifest = await utils.readImporterManifestOnly(importerDir)
  packageIsInstallable(importerDir, manifest as any, { // tslint:disable-line:no-any
    engineStrict: true,
    optional: false,
    pnpmVersion: packageManager.stableVersion,
    prefix: importerDir,
  })
  return manifest
}

export async function tryReadImporterManifest (importerDir: string): Promise<{
  fileName: string,
  manifest: ImporterManifest | null,
  writeImporterManifest: (manifest: ImporterManifest, force?: boolean) => Promise<void>,
}> {
  const { fileName, manifest, writeImporterManifest } = await utils.tryReadImporterManifest(importerDir)
  if (!manifest) return { fileName, manifest, writeImporterManifest }
  packageIsInstallable(importerDir, manifest as any, { // tslint:disable-line:no-any
    engineStrict: true,
    optional: false,
    pnpmVersion: packageManager.stableVersion,
    prefix: importerDir,
  })
  return { fileName, manifest, writeImporterManifest }
}
