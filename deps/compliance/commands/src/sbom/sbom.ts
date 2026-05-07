import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { packageManager } from '@pnpm/cli.meta'
import { docsUrl, readProjectManifestOnly } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { isSpdxLicenseExpression, resolveLicenseFromDir } from '@pnpm/deps.compliance.license-resolver'
import {
  collectSbomComponents,
  type SbomComponentType,
  type SbomFormat,
  serializeCycloneDx,
  serializeSpdx,
} from '@pnpm/deps.compliance.sbom'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, readWantedLockfile } from '@pnpm/lockfile.fs'
import { getStorePath } from '@pnpm/store.path'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export type SbomCommandOptions = {
  sbomFormat?: string
  sbomType?: string
  sbomSpecVersion?: string
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
  | 'virtualStoreDirMaxLength'
> & Pick<ConfigContext,
| 'selectedProjectsGraph'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
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
  'sbom-spec-version': String,
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
            description: 'The CycloneDX specification version (1.5, 1.6, or 1.7; default: 1.7)',
            name: '--sbom-spec-version <version>',
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
  const sbomSpecVersion = validateSbomSpecVersion(opts.sbomSpecVersion, format)

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
  // Keep the root in sync with transitive deps: consult manifest `license` /
  // legacy `licenses` first, then fall back to an on-disk LICENSE file. Drop
  // file-scanned values that aren't SPDX-valid to avoid non-compliant output.
  const rootLicenseInfo = await resolveLicenseFromDir({ manifest, dir: opts.dir })
  const rootLicense = rootLicenseInfo && rootLicenseInfo.name !== 'Unknown' &&
    (!rootLicenseInfo.licenseFile || isSpdxLicenseExpression(rootLicenseInfo.name))
    ? rootLicenseInfo.name
    : undefined
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
      specVersion: sbomSpecVersion,
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

// Versions whose schema is fully covered by what we currently emit
// (e.g. metadata.lifecycles requires CycloneDX 1.5+).
const SUPPORTED_CYCLONEDX_SPEC_VERSIONS = ['1.5', '1.6', '1.7']

function validateSbomSpecVersion (value: string | undefined, format: SbomFormat): string | undefined {
  if (value == null) return undefined
  if (format !== 'cyclonedx') {
    throw new PnpmError(
      'SBOM_SPEC_VERSION_UNSUPPORTED_FORMAT',
      'The --sbom-spec-version option is only supported with --sbom-format cyclonedx.'
    )
  }
  const normalized = value.trim()
  if (!SUPPORTED_CYCLONEDX_SPEC_VERSIONS.includes(normalized)) {
    throw new PnpmError(
      'SBOM_INVALID_SPEC_VERSION',
      `Invalid CycloneDX spec version "${value}". Supported versions: ${SUPPORTED_CYCLONEDX_SPEC_VERSIONS.join(', ')}.`
    )
  }
  return normalized
}
