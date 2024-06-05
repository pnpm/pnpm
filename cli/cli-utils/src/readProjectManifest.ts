import * as utils from '@pnpm/read-project-manifest'
import { type SupportedArchitectures, type ProjectManifest } from '@pnpm/types'
import { packageIsInstallable } from './packageIsInstallable'

export interface ReadProjectManifestOpts {
  engineStrict?: boolean
  packageManagerStrict?: boolean
  packageManagerStrictVersion?: boolean
  nodeVersion?: string
  supportedArchitectures?: SupportedArchitectures
}

interface BaseReadProjectManifestResult {
  fileName: string
  writeProjectManifest: (manifest: ProjectManifest, force?: boolean) => Promise<void>
}

export interface ReadProjectManifestResult extends BaseReadProjectManifestResult {
  manifest: ProjectManifest
}

export async function readProjectManifest (
  projectDir: string,
  opts: ReadProjectManifestOpts = {}
): Promise<ReadProjectManifestResult> {
  const { fileName, manifest, writeProjectManifest } = await utils.readProjectManifest(projectDir)
  packageIsInstallable(projectDir, manifest as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
  return { fileName, manifest, writeProjectManifest }
}

export async function readProjectManifestOnly (
  projectDir: string,
  opts: ReadProjectManifestOpts = {}
): Promise<ProjectManifest> {
  const manifest = await utils.readProjectManifestOnly(projectDir)
  packageIsInstallable(projectDir, manifest as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
  return manifest
}

export interface TryReadProjectManifestResult extends BaseReadProjectManifestResult {
  manifest: ProjectManifest | null
}

export async function tryReadProjectManifest (
  projectDir: string,
  opts: ReadProjectManifestOpts
): Promise<TryReadProjectManifestResult> {
  const { fileName, manifest, writeProjectManifest } = await utils.tryReadProjectManifest(projectDir)
  if (manifest == null) return { fileName, manifest, writeProjectManifest }
  packageIsInstallable(projectDir, manifest as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
  return { fileName, manifest, writeProjectManifest }
}
