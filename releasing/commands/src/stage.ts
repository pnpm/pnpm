import { createHash } from 'node:crypto'
import fs from 'node:fs/promises'
import path from 'node:path'
import { gunzipSync } from 'node:zlib'

import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { SyntheticOtpError, type WebAuthFetchOptions, withOtpHandling } from '@pnpm/network.web-auth'
import npa from '@pnpm/npm-package-arg'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import tar from 'tar-stream'

import * as publishCommand from './publish/publish.js'
import { createPublishContext, type PublishSummary } from './publish/publishPackedPkg.js'

const DEFAULT_REGISTRY = 'https://registry.npmjs.org/'
const PER_PAGE = 100
const UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
const STAGE_SUBCOMMANDS = ['publish', 'list', 'view', 'approve', 'reject', 'download'] as const

type StageSubcommand = typeof STAGE_SUBCOMMANDS[number]
type StageOptions = Parameters<typeof publishCommand.publish>[0] & {
  cliOptions?: Record<string, unknown>
  json?: boolean
  otp?: string
  registry?: string
}

interface StageItem {
  id?: string
  packageName?: string
  version?: string
  tag?: string
  createdAt?: string
  actor?: string
  actorType?: string
  shasum?: string
  [key: string]: unknown
}

interface StageListResponse {
  items: StageItem[]
  total: number
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...publishCommand.rcOptionsTypes(),
    ...pick([
      'registry',
    ], allTypes),
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return publishCommand.cliOptionsTypes()
}

export const commandNames = ['stage']

export const completion = async (_cliOpts: Record<string, unknown>, params: string[]): Promise<Array<{ name: string }>> => {
  if (params.length > 0) return []
  return STAGE_SUBCOMMANDS.map((name) => ({ name }))
}

export function help (): string {
  return renderHelp({
    description: 'Stage packages for publishing, deferring proof-of-presence (2FA) to a later point in time.',
    descriptionLists: [
      {
        title: 'Subcommands',
        list: [
          {
            description: 'Stage a package for publishing.',
            name: 'publish',
          },
          {
            description: 'List all staged package versions.',
            name: 'list',
          },
          {
            description: 'View details of a specific staged package.',
            name: 'view',
          },
          {
            description: 'Approve a staged package, publishing it to the npm registry.',
            name: 'approve',
          },
          {
            description: 'Reject a staged package, removing it from the registry.',
            name: 'reject',
          },
          {
            description: 'Download the tarball of a staged package for inspection.',
            name: 'download',
          },
        ],
      },
      {
        title: 'Options',
        list: [
          {
            description: 'The base URL of the npm registry.',
            name: '--registry <url>',
          },
          {
            description: 'Show information in JSON format for list, view, publish, and download.',
            name: '--json',
          },
          {
            description: 'Registers the staged package with the given tag. By default, the "latest" tag is used.',
            name: '--tag <tag>',
          },
          {
            description: 'Tells the registry whether the staged package should be public or restricted.',
            name: '--access <public|restricted>',
          },
          {
            description: 'Does everything stage publish would do except uploading to the registry.',
            name: '--dry-run',
          },
          {
            description: 'One-time password for approve and reject.',
            name: '--otp',
          },
          {
            description: 'Stage all publishable packages from the workspace.',
            name: '--recursive',
            shortAlias: '-r',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('stage'),
    usages: [
      'pnpm stage publish [<tarball>|<dir>] [--tag <tag>] [--access <public|restricted>] [options]',
      'pnpm stage list [<package-spec>]',
      'pnpm stage view <stage-id>',
      'pnpm stage approve <stage-id>',
      'pnpm stage reject <stage-id>',
      'pnpm stage download <stage-id>',
    ],
  })
}

export async function handler (
  opts: StageOptions,
  params: string[]
): Promise<{ exitCode?: number, output?: string } | string | undefined> {
  const subcommand = params[0] as StageSubcommand | undefined
  const subcommandParams = params.slice(1)

  switch (subcommand) {
    case 'publish':
      return stagePublish(opts, subcommandParams)
    case 'list':
      return stageList(opts, subcommandParams)
    case 'view':
      return stageView(opts, subcommandParams)
    case 'approve':
      return stageApprove(opts, subcommandParams)
    case 'reject':
      return stageReject(opts, subcommandParams)
    case 'download':
      return stageDownload(opts, subcommandParams)
    case undefined:
      throw new PnpmError('STAGE_SUBCOMMAND_REQUIRED', 'Stage subcommand is required', {
        hint: `Use one of: ${STAGE_SUBCOMMANDS.join(', ')}`,
      })
    default:
      throw new PnpmError('STAGE_UNKNOWN_SUBCOMMAND', `Unknown stage subcommand "${subcommand}"`, {
        hint: `Use one of: ${STAGE_SUBCOMMANDS.join(', ')}`,
      })
  }
}

async function stagePublish (
  opts: StageOptions,
  params: string[]
): Promise<{ exitCode?: number, output?: string } | string | undefined> {
  const result = await publishCommand.publish({
    ...opts,
    stage: true,
  }, params)

  if (opts.json) {
    if (result.publishSummary) {
      return { output: JSON.stringify(keyByPackageName([result.publishSummary]), null, 2), exitCode: 0 }
    }
    if (result.publishedPackages) {
      return { output: JSON.stringify(keyByPackageName(result.publishedPackages), null, 2), exitCode: result.exitCode ?? 0 }
    }
  }

  const publishedPackages = result.publishSummary
    ? [result.publishSummary]
    : result.publishedPackages ?? []
  if (publishedPackages.length > 0) {
    return {
      output: publishedPackages.map((summary) => renderStagePublishSummary(summary, { dryRun: opts.dryRun === true })).join('\n'),
      exitCode: result.exitCode ?? 0,
    }
  }
  if (result.exitCode) return { exitCode: result.exitCode }
  return undefined
}

async function stageList (opts: StageOptions, params: string[]): Promise<string> {
  let packageFilter: string | undefined
  if (params[0]) {
    const spec = parseStagePackageSpec(params[0])
    if (spec.rawSpec !== '' && spec.rawSpec !== '*') {
      throw new PnpmError('STAGE_VERSION_SPECIFIER_UNSUPPORTED', 'Version specifiers are not supported for listing staged packages')
    }
    packageFilter = spec.name
  }

  const context = createStageContext(opts, packageFilter)
  const items: StageItem[] = []
  let page = 0
  while (true) {
    const url = new URL('-/stage', context.registry)
    url.searchParams.set('page', page.toString())
    url.searchParams.set('perPage', PER_PAGE.toString())
    if (packageFilter) {
      url.searchParams.set('package', packageFilter)
    }
    // eslint-disable-next-line no-await-in-loop
    const res = await stageJsonRequest<StageListResponse>(context, { url: url.href, action: 'list staged packages' })
    items.push(...res.items)
    if (items.length >= res.total || res.items.length < PER_PAGE) break
    page++
  }

  if (opts.json) return JSON.stringify(items, null, 2)
  if (items.length === 0) {
    return packageFilter
      ? `No staged versions of package name "${packageFilter}".`
      : 'No staged packages found.'
  }
  return items.map(renderStageItem).join('\n\n')
}

async function stageView (opts: StageOptions, params: string[]): Promise<string> {
  const stageId = requireStageId(params, 'view')
  const context = createStageContext(opts)
  const item = await stageJsonRequest<StageItem>(context, {
    url: new URL(`-/stage/${stageId}`, context.registry).href,
    action: `view staged package ${stageId}`,
  })
  return opts.json ? JSON.stringify(item, null, 2) : renderStageItem(item)
}

async function stageApprove (opts: StageOptions, params: string[]): Promise<string> {
  const stageId = requireStageId(params, 'approve')
  const context = createStageContext(opts)
  await stageRequestWithOtp(context, {
    url: new URL(`-/stage/${stageId}/approve`, context.registry).href,
    init: { method: 'POST' },
    action: `approve staged package ${stageId}`,
  })
  return `Staged package ${stageId} approved and published successfully.`
}

async function stageReject (opts: StageOptions, params: string[]): Promise<string> {
  const stageId = requireStageId(params, 'reject')
  const context = createStageContext(opts)
  globalWarn('Rejecting will permanently delete this staged publish record and tarball from the registry.')
  await stageRequestWithOtp(context, {
    url: new URL(`-/stage/${stageId}`, context.registry).href,
    init: { method: 'DELETE' },
    action: `reject staged package ${stageId}`,
  })
  return `Staged package ${stageId} has been rejected.`
}

async function stageDownload (opts: StageOptions, params: string[]): Promise<string> {
  const stageId = requireStageId(params, 'download')
  const context = createStageContext(opts)
  const response = await stageRequest(context, {
    url: new URL(`-/stage/${stageId}/tarball`, context.registry).href,
    init: { method: 'GET' },
    action: `download staged package ${stageId}`,
  })
  const tarballData = Buffer.from(await response.arrayBuffer())
  const summary = await summarizeTarball(tarballData)
  const filename = `${normalizePackageName(summary.name)}-${summary.version}-${stageId}.tgz`
  const downloadedSummary = { ...summary, filename }
  await fs.writeFile(path.resolve(opts.dir ?? process.cwd(), filename), tarballData)

  if (opts.json) return JSON.stringify({ [summary.name]: downloadedSummary }, null, 2)
  return `${renderTarballSummary(downloadedSummary)}\n${filename}`
}

function keyByPackageName (packages: Array<PublishSummary | { name?: string, version?: string }>): Record<string, PublishSummary | { name?: string, version?: string }> {
  const keyed: Record<string, PublishSummary | { name?: string, version?: string }> = {}
  for (const pkg of packages) {
    const key = pkg.name ?? ('id' in pkg ? pkg.id : undefined)
    if (key) keyed[key] = pkg
  }
  return keyed
}

function renderStagePublishSummary (summary: PublishSummary | { name?: string, version?: string }, opts: { dryRun: boolean }): string {
  const id = 'id' in summary && summary.id
    ? summary.id
    : summary.name && summary.version
      ? `${summary.name}@${summary.version}`
      : summary.name ?? '<unknown package>'
  if (opts.dryRun) return `+ ${id} (would stage)`
  if ('stageId' in summary && summary.stageId) {
    return `+ ${id} (staged with id ${summary.stageId})`
  }
  return `+ ${id} (staged)`
}

function parseStagePackageSpec (rawSpec: string): { name: string, rawSpec: string } {
  let spec: ReturnType<typeof npa>
  try {
    spec = npa(rawSpec)
  } catch {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${rawSpec}`)
  }
  if (!spec.name) {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${rawSpec}`)
  }
  return { name: spec.name, rawSpec: spec.rawSpec }
}

function requireStageId (params: string[], subcommand: StageSubcommand): string {
  if (!params[0]) {
    throw new PnpmError('STAGE_ID_REQUIRED', `Missing required <stage-id> for "pnpm stage ${subcommand}"`)
  }
  const stageId = params[0]
  if (!UUID_REGEX.test(stageId)) {
    throw new PnpmError('INVALID_STAGE_ID', 'stage-id must be a valid UUID')
  }
  return stageId
}

interface StageContext {
  opts: StageOptions
  registry: string
  authHeaderValue: string | undefined
  fetchFromRegistry: ReturnType<typeof createFetchFromRegistry>
}

function createStageContext (opts: StageOptions, packageName?: string): StageContext {
  const registry = getStageRegistry(opts, packageName)
  const getAuthHeaderByUri = createGetAuthHeaderByURI(opts.configByUri ?? {} as Record<string, RegistryConfig>, registry)
  return {
    opts,
    registry,
    authHeaderValue: getAuthHeaderByUri(registry),
    fetchFromRegistry: createFetchFromRegistry(opts),
  }
}

function getStageRegistry (opts: StageOptions, packageName?: string): string {
  const registries = getRegistries(opts)
  const registry = packageName
    ? pickRegistryForPackage(registries, packageName)
    : registries.default
  return normalizeRegistryUrl(registry)
}

function getRegistries (opts: StageOptions): Registries {
  return opts.registries ?? { default: opts.registry ?? DEFAULT_REGISTRY }
}

function normalizeRegistryUrl (registry: string): string {
  return registry.endsWith('/') ? registry : `${registry}/`
}

interface StageRequestParams {
  url: string
  action: string
  init?: StageRequestInit
  otp?: string
}

async function stageJsonRequest<T> (context: StageContext, params: { url: string, action: string }): Promise<T> {
  const response = await stageRequest(context, { url: params.url, action: params.action, init: { method: 'GET' } })
  return await response.json() as T
}

async function stageRequestWithOtp (
  context: StageContext,
  params: { url: string, init: StageRequestInit, action: string }
): Promise<Response> {
  return withOtpHandling({
    context: createPublishContext(context.opts),
    fetchOptions: createWebAuthFetchOptions(context.opts),
    operation: async (otp) => stageRequest(context, {
      url: params.url,
      action: params.action,
      init: params.init,
      otp: otp ?? getConfiguredOtp(context.opts),
    }),
  })
}

async function stageRequest (context: StageContext, params: StageRequestParams): Promise<Response> {
  const init = params.init ?? { method: 'GET' }
  const response = await context.fetchFromRegistry(params.url, {
    authHeaderValue: context.authHeaderValue,
    body: init.body,
    fullMetadata: true,
    headers: {
      'npm-auth-type': 'web',
      'npm-command': 'stage',
      ...init.headers,
      ...(params.otp != null ? { 'npm-otp': params.otp } : {}),
    },
    method: init.method,
    timeout: context.opts.fetchTimeout,
  })
  if (!response.ok) {
    await throwOnErrorResponse(response, params.action)
  }
  return response
}

interface StageRequestInit {
  body?: string
  headers?: Record<string, string>
  method: 'DELETE' | 'GET' | 'POST'
}

function getConfiguredOtp (opts: StageOptions): string | undefined {
  if (typeof opts.otp === 'string') return opts.otp
  const cliOtp = opts.cliOptions?.otp
  return typeof cliOtp === 'string' ? cliOtp : undefined
}

function createWebAuthFetchOptions (opts: StageOptions): WebAuthFetchOptions {
  return {
    method: 'GET',
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  }
}

async function throwOnErrorResponse (response: Response, action: string): Promise<never> {
  let text = ''
  try {
    text = await response.text()
  } catch {}
  let parsed: unknown
  try {
    parsed = text ? JSON.parse(text) : undefined
  } catch {}

  if (response.status === 401 && isOtpChallenge(response, parsed)) {
    throw SyntheticOtpError.fromUnknownBody(globalWarn, parsed)
  }
  throw new StageRegistryError({
    action,
    status: response.status,
    statusText: response.statusText,
    text,
  })
}

function isOtpChallenge (response: Response, body: unknown): boolean {
  if (hasWebAuthUrls(body)) return true
  const wwwAuthenticate = response.headers.get('www-authenticate')?.toLowerCase()
  return wwwAuthenticate?.includes('otp') === true
}

function hasWebAuthUrls (body: unknown): boolean {
  if (body == null || typeof body !== 'object') return false
  const record = body as Record<string, unknown>
  return typeof record.authUrl === 'string' && typeof record.doneUrl === 'string'
}

class StageRegistryError extends PnpmError {
  readonly statusCode: number
  readonly status: number
  readonly statusText: string
  readonly text: string

  constructor (opts: { action: string, status: number, statusText: string, text: string }) {
    const statusDisplay = opts.statusText ? `${opts.status} ${opts.statusText}` : opts.status.toString()
    const text = opts.text.trim()
    super('STAGE_REGISTRY_ERROR', `Failed to ${opts.action} (status ${statusDisplay})${text ? `: ${text}` : ''}`)
    this.statusCode = opts.status
    this.status = opts.status
    this.statusText = opts.statusText
    this.text = opts.text
  }
}

async function summarizeTarball (tarballData: Buffer): Promise<PublishSummary> {
  const extract = tar.extract()
  const files: Array<{ path: string }> = []
  const bundled = new Set<string>()
  let manifest: { _id?: string, name?: string, version?: string, bundledDependencies?: unknown, bundleDependencies?: unknown, dependencies?: Record<string, unknown> } | undefined
  let entryCount = 0
  let unpackedSize = 0

  await new Promise<void>((resolve, reject) => {
    extract.on('entry', (header, stream, next) => {
      const chunks: Buffer[] = []
      const isFile = header.type === 'file'
      if (isFile) {
        entryCount++
        unpackedSize += header.size ?? 0
        files.push({ path: header.name.replace(/^package\//, '') })
        const bundledMatch = /^package\/node_modules\/((?:@[^/]+\/)?[^/]+)/.exec(header.name)
        if (bundledMatch?.[1]) {
          bundled.add(bundledMatch[1])
        }
      }
      if (header.name === 'package/package.json') {
        stream.on('data', (chunk: Buffer) => chunks.push(Buffer.from(chunk)))
      }
      stream.on('error', reject)
      stream.on('end', () => {
        if (header.name === 'package/package.json') {
          try {
            manifest = JSON.parse(Buffer.concat(chunks).toString())
          } catch (error: unknown) {
            reject(error)
            return
          }
        }
        next()
      })
      stream.resume()
    })
    extract.on('error', reject)
    extract.on('finish', resolve)
    extract.end(maybeGunzip(tarballData))
  })

  if (!manifest?.name || !manifest.version) {
    throw new PnpmError('STAGE_TARBALL_MANIFEST_NOT_FOUND', 'Could not read package.json from tarball')
  }

  const shasum = createHash('sha1').update(tarballData).digest('hex')
  const integrity = `sha512-${createHash('sha512').update(tarballData).digest('base64')}`
  files.sort((a, b) => a.path.localeCompare(b.path, 'en'))
  return {
    id: manifest._id ?? `${manifest.name}@${manifest.version}`,
    name: manifest.name,
    version: manifest.version,
    size: tarballData.byteLength,
    unpackedSize,
    shasum,
    integrity,
    filename: `${normalizePackageName(manifest.name)}-${manifest.version}.tgz`,
    files,
    entryCount,
    bundled: bundled.size > 0 ? Array.from(bundled).sort() : extractBundledDependencies(manifest),
  }
}

function maybeGunzip (tarballData: Buffer): Buffer {
  try {
    return gunzipSync(tarballData)
  } catch {
    return tarballData
  }
}

function extractBundledDependencies (manifest: { bundledDependencies?: unknown, bundleDependencies?: unknown, dependencies?: Record<string, unknown> }): string[] {
  const raw = manifest.bundledDependencies ?? manifest.bundleDependencies
  if (!raw) return []
  if (Array.isArray(raw)) return raw.filter((name): name is string => typeof name === 'string')
  if (raw === true) return Object.keys(manifest.dependencies ?? {})
  return []
}

function renderStageItem (item: StageItem): string {
  const { id, packageName, version, tag, createdAt, actor, actorType, shasum, ...rest } = item
  return renderKeyValues({
    id,
    'package name': packageName,
    version,
    tag,
    'date staged': createdAt,
    'staged by': actorType ? `${actor ?? ''} (${actorType})` : actor,
    shasum,
    ...rest,
  })
}

function renderTarballSummary (summary: PublishSummary): string {
  return `package: ${summary.name}@${summary.version}
Tarball Contents
${summary.files.map(({ path }) => path).join('\n')}
Tarball Details
name: ${summary.name}
version: ${summary.version}
filename: ${summary.filename}
package size: ${summary.size}
unpacked size: ${summary.unpackedSize}
shasum: ${summary.shasum}
integrity: ${summary.integrity}
total files: ${summary.entryCount}`
}

function renderKeyValues (values: Record<string, unknown>): string {
  return Object.entries(values)
    .flatMap(([key, value]) => value == null ? [] : [`${key}: ${renderValue(value)}`])
    .join('\n')
}

function renderValue (value: unknown): string {
  return typeof value === 'object' ? JSON.stringify(value) : String(value)
}

function normalizePackageName (name: string): string {
  return name.replace('@', '').replace('/', '-')
}
