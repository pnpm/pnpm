import fs from 'node:fs'
import path from 'node:path'

import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import type { SbomComponentType, SbomFormat } from '@pnpm/deps.compliance.sbom'
import { PnpmError } from '@pnpm/error'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { generateSbomForProject, type SerializeOptions } from './generateSbomForProject.js'
import { buildSharedContext, type SharedContext } from './sharedContext.js'

export type SbomCommandOptions = {
  sbomFormat?: string
  sbomType?: string
  sbomSpecVersion?: string
  lockfileOnly?: boolean
  sbomAuthors?: string
  sbomSupplier?: string
  out?: string
  split?: boolean
  excludePeers?: boolean
} & Pick<
  Config,
  | 'dev'
  | 'dir'
  | 'lockfileDir'
  | 'registriesByScope'
  | 'registriesByPrefix'
  | 'optional'
  | 'production'
  | 'resolvePeersFromWorkspaceRoot'
  | 'storeDir'
  | 'supportedArchitectures'
  | 'virtualStoreDir'
  | 'modulesDir'
  | 'pnpmHomeDir'
  | 'virtualStoreDirMaxLength'
> & Pick<ConfigContext,
| 'allProjectsGraph'
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
  out: String,
  split: Boolean,
  'exclude-peers': Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['sbom']

export const recursiveByDefault = true

const SBOM_OPTIONS_HELP = [
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
  {
    description: 'Write SBOM to a file instead of stdout. Use %s for the package name and %v for the version.',
    name: '--out <path>',
  },
  {
    description: 'Generate a separate SBOM for each matched workspace package. Outputs NDJSON to stdout, or files when combined with --out.',
    name: '--split',
  },
  {
    description: 'Exclude peer dependencies (and their exclusive transitive subtrees)',
    name: '--exclude-peers',
  },
]

export function help (): string {
  return renderHelp({
    description: 'Generate a Software Bill of Materials (SBOM) for the project.',
    descriptionLists: [
      {
        title: 'Options',
        list: SBOM_OPTIONS_HELP,
      },
      FILTERING,
    ],
    url: docsUrl('sbom'),
    usages: [
      'pnpm sbom --sbom-format cyclonedx',
      'pnpm sbom --sbom-format spdx',
      'pnpm sbom --sbom-format cyclonedx --lockfile-only',
      'pnpm sbom --sbom-format spdx --prod',
      'pnpm sbom --sbom-format cyclonedx --filter ./apps/my-app',
      'pnpm sbom --sbom-format cyclonedx --out out/%s.cdx.json',
      'pnpm sbom --sbom-format cyclonedx --split',
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

  const ctx = await buildSharedContext(opts)
  const serialOpts = { format, sbomType, sbomSpecVersion }
  // `%s` in --out only implies per-package output inside a workspace; in a
  // single-project repo it is interpolated from the root component on the
  // single-output path below.
  const hasWorkspaceGraph = opts.selectedProjectsGraph != null || opts.allProjectsGraph != null
  const shouldSplit = opts.split || (opts.out != null && opts.out.includes('%s') && hasWorkspaceGraph)

  if (shouldSplit) {
    return handleSplit(opts, serialOpts, ctx)
  }

  const { output, rootName, rootVersion } = await generateSbomForProject(opts, serialOpts, ctx)

  if (opts.out) {
    const filePath = opts.out
      .replaceAll('%s', sanitizePathSegment(sanitizePackageName(rootName)))
      .replaceAll('%v', sanitizePathSegment(rootVersion))
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    fs.writeFileSync(filePath, output)
    return { output: filePath, exitCode: 0 }
  }

  return { output, exitCode: 0 }
}

async function handleSplit (
  opts: SbomCommandOptions,
  serialOpts: SerializeOptions,
  ctx: SharedContext
): Promise<{ output: string, exitCode: number }> {
  const projectsGraph = getSplitProjectsGraph(opts)

  const ndjsonLines: string[] = []
  const splitFiles: SplitFiles | undefined = opts.out
    ? { outPattern: opts.out, files: [], writtenPaths: new Set(), createdDirs: new Set() }
    : undefined
  const compact = !opts.out

  for (const [dir, entry] of Object.entries(projectsGraph)) {
    const manifest = entry.package.manifest
    if (!manifest.name) continue

    const singleProjectGraph = { [dir as keyof typeof projectsGraph]: entry }

    // eslint-disable-next-line no-await-in-loop -- one project at a time keeps a single project's component graph and license lookups in memory
    const { output } = await generateSbomForProject(
      { ...opts, selectedProjectsGraph: singleProjectGraph as typeof projectsGraph, allProjectsGraph: undefined, split: false, out: undefined },
      serialOpts,
      ctx,
      compact
    )

    if (splitFiles) {
      writeSplitSbomFile(splitFiles, { name: manifest.name, version: manifest.version }, output)
    } else {
      ndjsonLines.push(output)
    }
  }

  if (splitFiles) {
    return {
      output: `Generated ${splitFiles.files.length} SBOMs:\n${splitFiles.files.map((file) => `  ${file}`).join('\n')}`,
      exitCode: 0,
    }
  }

  return { output: ndjsonLines.join('\n'), exitCode: 0 }
}

function getSplitProjectsGraph (opts: SbomCommandOptions): NonNullable<SbomCommandOptions['allProjectsGraph']> {
  const projectsGraph = opts.selectedProjectsGraph ?? opts.allProjectsGraph
  if (!projectsGraph) {
    throw new PnpmError(
      'SBOM_NO_PROJECTS',
      'No workspace projects found. --split requires a workspace.'
    )
  }

  if (opts.out && !opts.out.includes('%s')) {
    throw new PnpmError(
      'SBOM_OUT_MISSING_PLACEHOLDER',
      'When using --split with --out, the path must contain %s as a placeholder for the package name.'
    )
  }

  return projectsGraph
}

interface SplitFiles {
  outPattern: string
  files: string[]
  writtenPaths: Set<string>
  createdDirs: Set<string>
}

function writeSplitSbomFile (
  splitFiles: SplitFiles,
  manifest: { name: string, version?: string },
  output: string
): void {
  const filePath = splitFiles.outPattern
    .replaceAll('%s', sanitizePathSegment(sanitizePackageName(manifest.name)))
    .replaceAll('%v', sanitizePathSegment(manifest.version ?? '0.0.0'))
  // Sanitizing package names is lossy (e.g. "@a/b" and "a-b" both map to "a-b"),
  // so two packages can resolve to the same path. Fail loudly instead of letting
  // one SBOM silently overwrite another.
  if (splitFiles.writtenPaths.has(filePath)) {
    throw new PnpmError(
      'SBOM_OUT_PATH_COLLISION',
      `Multiple workspace packages resolve to the same output path "${filePath}". Include %v in the --out pattern to disambiguate.`
    )
  }
  splitFiles.writtenPaths.add(filePath)
  const fileDir = path.dirname(filePath)
  if (!splitFiles.createdDirs.has(fileDir)) {
    fs.mkdirSync(fileDir, { recursive: true })
    splitFiles.createdDirs.add(fileDir)
  }
  fs.writeFileSync(filePath, output)
  splitFiles.files.push(filePath)
}

function validateSbomType (value: string | undefined): SbomComponentType {
  if (!value || value === 'library') return 'library'
  if (value === 'application') return 'application'
  throw new PnpmError(
    'SBOM_INVALID_TYPE',
    `Invalid SBOM type "${value}". Use "library" or "application".`
  )
}

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

function sanitizePackageName (name: string): string {
  return name.replace(/^@/, '').replace(/\//g, '-')
}

function sanitizePathSegment (value: string): string {
  // Control characters (e.g. newlines) would produce confusing filenames and could
  // inject extra lines into the printed `--split --out` summary; filesystem
  // metacharacters are replaced so the value stays a single path segment.
  // eslint-disable-next-line no-control-regex -- matching control characters is the point of this pattern
  const sanitized = value.replace(/[/\\:*?"<>|\x00-\x1F\x7F]/g, '-')
  // `.`, `..`, or a blank value would let a crafted name/version escape or replace
  // the intended output directory once interpolated into an `--out` template.
  return sanitized === '.' || sanitized === '..' || sanitized.trim() === '' ? '-' : sanitized
}
