import { createHash } from 'node:crypto'
import fs from 'node:fs/promises'
import path from 'node:path'

import type { Config, ConfigContext } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import type { RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { type WebAuthFetchOptions, withOtpHandling } from '@pnpm/network.web-auth'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import type { Project } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import npmFetch from 'npm-registry-fetch'
import { realpathMissing } from 'realpath-missing'
import semver from 'semver'
import { temporaryDirectory } from 'tempy'

import { createPublishSummary, type PublishSummary } from '../tarball/publishSummary.js'
import * as pack from './pack.js'
import { runScriptsIfPresent } from './publish.js'
import {
  createPublishContext,
  createPublishOptions,
  findRegistryInfo,
  isPublishAccess,
} from './publishPackedPkg.js'
import type { PublishRecursiveOpts } from './recursivePublish.js'

export const BATCH_PUBLISH_ENDPOINT = '/-/pnpm/v1/publish'

export type BatchPublishOptions = PublishRecursiveOpts
& Pick<Config, 'embedReadme' | 'ignoreScripts' | 'nodeLinker' | 'packGzipLevel' | 'skipManifestObfuscation'>
& Partial<Pick<ConfigContext, 'hooks'>>

interface PackedPkg {
  project: Project
  publishedManifest: ExportedManifest
  tarballData: Buffer
  summary: PublishSummary
}

/**
 * Publish all {@link pkgs} by sending a single `PUT /-/pnpm/v1/publish` request per target
 * registry, instead of one request per package. The endpoint is not part of the standard npm
 * registry API — the registry has to implement it explicitly (pnpr does), so this whole code
 * path is opt-in via the `--batch` flag.
 *
 * Every package is packed (and its `prepublishOnly`/`prepublish` scripts run) before anything
 * is sent, so a package that fails to pack aborts the publish before any package reaches the
 * registry. The `publish`/`postpublish` scripts run only after the requests succeeded.
 *
 * @param pkgs packages to publish, already filtered (no private packages, no already-published
 *   versions) and in dependency order.
 */
export async function batchPublishPackages (pkgs: Project[], opts: BatchPublishOptions): Promise<PublishSummary[]> {
  if (opts.stage) {
    throw new PnpmError('BATCH_PUBLISH_NO_STAGE', 'Staged publishing cannot be combined with --batch')
  }
  if (opts.provenance) {
    throw new PnpmError('BATCH_PUBLISH_NO_PROVENANCE', 'Provenance statements cannot be generated when publishing with --batch', {
      hint: 'Provenance is bound to a single package, but --batch sends many packages in one request. Publish without --batch to attach provenance.',
    })
  }
  const packedByRegistry = new Map<string, PackedPkg[]>()
  const packedPkgs: PackedPkg[] = []
  for (const project of pkgs) {
    // eslint-disable-next-line no-await-in-loop
    const packedPkg = await packPkgForBatch(project, opts)
    const publishConfigRegistry = typeof packedPkg.publishedManifest.publishConfig?.registry === 'string'
      ? packedPkg.publishedManifest.publishConfig.registry
      : undefined
    const { registry } = findRegistryInfo(packedPkg.publishedManifest, opts, publishConfigRegistry)
    let group = packedByRegistry.get(registry!)
    if (!group) {
      group = []
      packedByRegistry.set(registry!, group)
    }
    group.push(packedPkg)
    packedPkgs.push(packedPkg)
  }
  for (const [registry, group] of packedByRegistry.entries()) {
    for (const { summary } of group) {
      globalInfo(`📦 ${summary.id} → ${registry}`)
    }
    if (opts.dryRun) {
      globalWarn(`Skip publishing ${group.length} package(s) to ${registry} (dry run)`)
      continue
    }
    // eslint-disable-next-line no-await-in-loop
    await multiPublishToRegistry(registry, group, opts)
    globalInfo(`✅ Published ${group.length} package(s) to ${registry} in a single request`)
  }
  if (!opts.ignoreScripts) {
    for (const { project } of packedPkgs) {
      // eslint-disable-next-line no-await-in-loop
      await runScriptsIfPresent(await lifecycleOpts(project.rootDir, opts), ['publish', 'postpublish'], project.manifest)
    }
  }
  return packedPkgs.map(({ summary }) => summary)
}

async function packPkgForBatch (project: Project, opts: BatchPublishOptions): Promise<PackedPkg> {
  if (!opts.ignoreScripts) {
    await runScriptsIfPresent(await lifecycleOpts(project.rootDir, opts), ['prepublishOnly', 'prepublish'], project.manifest)
  }
  // The tarball is packed into a temporary directory and read into memory right away — the
  // request body carries it base64-encoded, so nothing needs to stay on disk.
  const packDestination = temporaryDirectory()
  try {
    const packResult = await pack.api({
      ...opts,
      dir: project.rootDir,
      packDestination,
      dryRun: false,
    })
    const tarballData = await fs.readFile(packResult.tarballPath)
    return {
      project,
      publishedManifest: packResult.publishedManifest,
      tarballData,
      summary: createPublishSummary(packResult, tarballData),
    }
  } finally {
    await rimraf(packDestination)
  }
}

async function lifecycleOpts (pkgRoot: string, opts: BatchPublishOptions): Promise<RunLifecycleHookOptions> {
  return {
    depPath: pkgRoot,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: opts.extraEnv,
    pkgRoot,
    rootModulesDir: await realpathMissing(path.join(pkgRoot, 'node_modules')),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
    userAgent: opts.userAgent,
  }
}

async function multiPublishToRegistry (registry: string, group: PackedPkg[], opts: BatchPublishOptions): Promise<void> {
  // A package-scoped OIDC token cannot authorize a request that publishes many packages, so the
  // OIDC exchange is skipped and only statically configured credentials are used.
  const publishOptions = await createPublishOptions(group[0].publishedManifest, opts, { oidc: false })
  const body = {
    packages: group.map((packedPkg) => createPublishDocument(packedPkg, registry, opts)),
  }
  const fetchOptions: WebAuthFetchOptions = {
    method: 'GET',
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  }
  try {
    await withOtpHandling({
      context: createPublishContext(opts),
      fetchOptions,
      operation: async (otp) => npmFetch(BATCH_PUBLISH_ENDPOINT, {
        ...publishOptions,
        access: publishOptions.access ?? undefined,
        otp,
        method: 'PUT',
        body,
        ignoreBody: true,
      } as npmFetch.FetchOptions),
    })
  } catch (err) {
    const statusCode = (err as { statusCode?: number }).statusCode
    if (statusCode === 404 || statusCode === 405) {
      throw new PnpmError(
        'BATCH_PUBLISH_UNSUPPORTED',
        `The registry at ${registry} does not support publishing multiple packages in a single request`,
        {
          hint: `Retry without the --batch flag, or publish to a registry that implements "PUT ${BATCH_PUBLISH_ENDPOINT}" (for example, pnpr).`,
        }
      )
    }
    throw err
  }
}

interface PublishDocument {
  _id: string
  name: string
  description?: string
  'dist-tags': Record<string, string>
  versions: Record<string, ExportedManifest>
  access: 'public' | 'restricted' | null
  _attachments: Record<string, {
    content_type: string
    data: string
    length: number
  }>
}

/**
 * Build the npm publish document for one package — the same JSON body `libnpmpublish` would
 * send as the whole `PUT /:pkg` request. The batch publish request body is an array of these,
 * which lets the registry reuse its single-package publish pipeline per entry.
 */
function createPublishDocument (
  { publishedManifest, tarballData }: Pick<PackedPkg, 'publishedManifest' | 'tarballData'>,
  registry: string,
  opts: Pick<BatchPublishOptions, 'access' | 'tag'>
): PublishDocument {
  const name = publishedManifest.name as string
  const version = semver.clean(publishedManifest.version as string)
  if (!version) {
    throw new PnpmError('BAD_SEMVER', `Invalid semver: ${publishedManifest.version as string}`)
  }
  const publishConfigAccess = publishedManifest.publishConfig?.access
  const access = opts.access ?? (isPublishAccess(publishConfigAccess) ? publishConfigAccess : null)
  if (!name.startsWith('@') && access === 'restricted') {
    throw new PnpmError('UNSCOPED_RESTRICTED', `Can't restrict access to the unscoped package ${name}`)
  }
  const tarballName = `${name}-${version}.tgz`
  const manifest: ExportedManifest = {
    ...publishedManifest,
    _id: `${name}@${version}`,
    version,
    _nodeVersion: process.versions.node,
    dist: {
      integrity: `sha512-${createHash('sha512').update(tarballData).digest('base64')}`,
      shasum: createHash('sha1').update(tarballData).digest('hex'),
      // libnpmpublish stores an http:// URL on purpose (clients fetch via HTTPS regardless when
      // the registry is HTTPS); keep the wire format identical to a single-package publish.
      tarball: new URL(`${name}/-/${tarballName}`, registry).href.replace(/^https:\/\//, 'http://'),
    },
  } as ExportedManifest
  return {
    _id: name,
    name,
    description: publishedManifest.description,
    'dist-tags': { [opts.tag ?? 'latest']: version },
    versions: { [version]: manifest },
    access,
    _attachments: {
      [tarballName]: {
        content_type: 'application/octet-stream',
        data: tarballData.toString('base64'),
        length: tarballData.length,
      },
    },
  }
}
