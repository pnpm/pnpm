import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, readWantedLockfile } from '@pnpm/lockfile.fs'
import { getStorePath } from '@pnpm/store-path'
import { packageManager } from '@pnpm/cli-meta'
import {
  collectSbomComponents,
  serializeCycloneDx,
  serializeSpdx,
  type SbomFormat,
  type SbomComponentType,
} from '@pnpm/sbom'
import { pick } from 'ramda'
import renderHelp from 'render-help'

export type SbomCommandOptions = {
  sbomFormat?: string
  sbomType?: string
  lockfileOnly?: boolean
  sbomAuthors?: string
  sbomSupplier?: string
} & Pick<
  Config,
  | 'dev'
  | 'dir'
  | 'lockfileDir'
  | 'registries'
  | 'optional'
  | 'production'
  | 'storeDir'
  | 'virtualStoreDir'
  | 'modulesDir'
  | 'pnpmHomeDir'
  | 'selectedProjectsGraph'
  | 'rootProjectManifest'
  | 'rootProjectManifestDir'
  | 'virtualStoreDirMaxLength'
> &
Partial<Pick<Config, 'userConfig'>>

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(
    ['dev', 'global-dir', 'global', 'optional', 'production', 'store-dir'],
    allTypes
  )
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
  'sbom-format': String,
  'sbom-type': String,
  'sbom-authors': String,
  'sbom-supplier': String,
  'lockfile-only': Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['sbom']

export function help (): string {
  return renderHelp({
    description: 'Generate a Software Bill of Materials (SBOM) for the project.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'The SBOM output format (required)',
            name: '--sbom-format <cyclonedx|spdx>',
          },
          {
            description: 'The component type for the root package (default: library)',
            name: '--sbom-type <library|application>',
          },
          {
            description: 'Only use lockfile data (skip reading from the store)',
            name: '--lockfile-only',
          },
          {
            description: 'Comma-separated list of SBOM authors (CycloneDX metadata.authors)',
            name: '--sbom-authors <names>',
          },
          {
            description: 'SBOM supplier name (CycloneDX metadata.supplier)',
            name: '--sbom-supplier <name>',
          },
          {
            description: 'Only include "dependencies" and "optionalDependencies"',
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Only include "devDependencies"',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Don\'t include "optionalDependencies"',
            name: '--no-optional',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('sbom'),
    usages: [
      'pnpm sbom --sbom-format cyclonedx',
      'pnpm sbom --sbom-format spdx',
      'pnpm sbom --sbom-format cyclonedx --lockfile-only',
      'pnpm sbom --sbom-format spdx --prod',
    ],
  })
}

export async function handler (
  opts: SbomCommandOptions,
  _params: string[] = []
): Promise<{ output: string, exitCode: number }> {
  if (!opts.sbomFormat) {
    throw new PnpmError(
      'SBOM_NO_FORMAT',
      'The --sbom-format option is required. Use --sbom-format cyclonedx or --sbom-format spdx.',
      { hint: help() }
    )
  }

  const format = opts.sbomFormat.toLowerCase() as SbomFormat
  if (format !== 'cyclonedx' && format !== 'spdx') {
    throw new PnpmError(
      'SBOM_INVALID_FORMAT',
      `Invalid SBOM format "${opts.sbomFormat}". Use "cyclonedx" or "spdx".`
    )
  }

  const sbomType = validateSbomType(opts.sbomType)

  const lockfile = await readWantedLockfile(opts.lockfileDir ?? opts.dir, {
    ignoreIncompatible: true,
  })

  if (lockfile == null) {
    throw new PnpmError(
      'SBOM_NO_LOCKFILE',
      `No ${WANTED_LOCKFILE} found: Cannot generate SBOM without a lockfile`
    )
  }

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  const manifest = await readProjectManifestOnly(opts.dir)
  const rootName = manifest.name ?? 'unknown'
  const rootVersion = manifest.version ?? '0.0.0'
  const rootLicense = typeof manifest.license === 'string' ? manifest.license : undefined
  const rootAuthor = typeof manifest.author === 'string'
    ? manifest.author
    : (manifest.author as { name?: string } | undefined)?.name
  const rootRepository = typeof manifest.repository === 'string'
    ? manifest.repository
    : (manifest.repository as { url?: string } | undefined)?.url

  const includedImporterIds = opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph)
      .map((p) => getLockfileImporterId(opts.lockfileDir ?? opts.dir, p))
    : undefined

  let storeDir: string | undefined
  if (!opts.lockfileOnly) {
    storeDir = await getStorePath({
      pkgRoot: opts.dir,
      storePath: opts.storeDir,
      pnpmHomeDir: opts.pnpmHomeDir,
    })
  }

  const result = await collectSbomComponents({
    lockfile,
    rootName,
    rootVersion,
    rootLicense,
    rootDescription: manifest.description,
    rootAuthor,
    rootRepository,
    sbomType,
    include,
    registries: opts.registries,
    lockfileDir: opts.lockfileDir ?? opts.dir,
    includedImporterIds,
    lockfileOnly: opts.lockfileOnly,
    storeDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
  })

  const output = format === 'cyclonedx'
    ? serializeCycloneDx(result, {
      pnpmVersion: packageManager.version,
      lockfileOnly: opts.lockfileOnly,
      sbomAuthors: opts.sbomAuthors?.split(',').map((s) => s.trim()).filter(Boolean),
      sbomSupplier: opts.sbomSupplier,
    })
    : serializeSpdx(result)

  return { output, exitCode: 0 }
}

function validateSbomType (value: string | undefined): SbomComponentType {
  if (!value || value === 'library') return 'library'
  if (value === 'application') return 'application'
  throw new PnpmError(
    'SBOM_INVALID_TYPE',
    `Invalid SBOM type "${value}". Use "library" or "application".`
  )
}
