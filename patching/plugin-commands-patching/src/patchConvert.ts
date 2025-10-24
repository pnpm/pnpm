import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import path from 'path'
import fs from 'fs'
import { updatePatchedDependencies } from './updatePatchedDependencies.js'
import { install } from '@pnpm/plugin-commands-installation'
import renderHelp from 'render-help'
import pick from 'ramda/src/pick'
import { docsUrl } from '@pnpm/cli-utils'

async function validateAndGetPatchFiles (folderPath: string): Promise<string[]> {
  if (!fs.existsSync(folderPath)) {
    throw new PnpmError('FOLDER_NOT_FOUND', `Folder not found: ${folderPath}`)
  }

  const stat = await fs.promises.stat(folderPath)
  if (!stat.isDirectory()) {
    throw new PnpmError('NOT_A_DIRECTORY', `Path is not a directory: ${folderPath}`)
  }

  const files = await fs.promises.readdir(folderPath)
  const patchFiles = files.filter((file: string) => file.endsWith('.patch'))

  if (patchFiles.length === 0) {
    throw new PnpmError('NO_PATCH_FILES', `No patch files found in directory: ${folderPath}`)
  }

  return patchFiles.map((file: string) => path.join(folderPath, file))
}

const NODE_MODULES_PATTERNS = [
  'diff --git a/node_modules/',
  '--- a/node_modules/',
  '+++ b/node_modules/',
] as const

function needsConversion (content: string): boolean {
  return NODE_MODULES_PATTERNS.some(pattern => content.includes(pattern))
}

function generateReplacePattern (patchFilePath: string): string {
  const baseName = path.basename(patchFilePath)
  const packageInfo = baseName.split('+').slice(0, -1).join('/')
  return `/node_modules/${packageInfo}`.replace(/\\/g, '/')
}

async function convertContentAndFileName (patchFilePath: string): Promise<string> {
  const patchContent = await fs.promises.readFile(patchFilePath, 'utf8')

  if (!needsConversion(patchContent)) {
    return ''
  }

  const replacePattern = generateReplacePattern(patchFilePath)
  const convertedContent = patchContent.replace(new RegExp(replacePattern, 'g'), '')
  const outputPath = convertPatchNameToPnpmFormat(patchFilePath)

  await fs.promises.writeFile(outputPath, convertedContent, 'utf8')
  await fs.promises.unlink(patchFilePath)
  return outputPath
}

function convertPatchNameToPnpmFormat (patchFileName: string): string {
  const parts = patchFileName.split('+')
  const version = parts.pop()
  return `${parts.join('__')}@${version}`
}

function convertedPathToPatchedDependencyKeyValue (convertedPath: string): [string, string] {
  const baseName = path.basename(convertedPath)
  const normalizedPath = convertedPath.replace(/\\/g, '/')
  const patchesDir = path.dirname(normalizedPath).split('/').pop()!
  const key = baseName.replace(/\.patch$/, '').replace(/__/g, '/')
  return [key, `${patchesDir}/${baseName}`]
}

function validatePatchFile (patchFilePath: string): void {
  if (!fs.existsSync(patchFilePath)) {
    throw new PnpmError('FILE_NOT_FOUND', `Patch file not found: ${patchFilePath}`)
  }
  if (!patchFilePath.endsWith('.patch')) {
    throw new PnpmError('INVALID_PATCH_FILE', `File must have .patch extension: ${patchFilePath}`)
  }
  if (!patchFilePath.includes('+')) {
    throw new PnpmError('INVALID_PATCH_FILE_NAME', `Invalid patch file name: expected '+' in the file name (e.g. pkg+1.0.0.patch): ${patchFilePath}`)
  }
}

function checkOutputExists (patchFilePath: string): void {
  const convertedName = convertPatchNameToPnpmFormat(path.basename(patchFilePath))
  const outputPath = path.join(path.dirname(patchFilePath), convertedName)
  if (fs.existsSync(outputPath)) {
    throw new PnpmError('PATCH_ALREADY_EXISTS', `Converted patch file already exists: ${convertedName}`)
  }
}

async function convertSinglePatchFile (patchFilePath: string): Promise<Array<[string, string]>> {
  validatePatchFile(patchFilePath)
  checkOutputExists(patchFilePath)

  const convertedPath = await convertContentAndFileName(patchFilePath)
  return convertedPath ? [convertedPathToPatchedDependencyKeyValue(convertedPath)] : []
}

async function convertPatchFile (patchPath: string): Promise<Array<[string, string]>> {
  if (!fs.existsSync(patchPath)) {
    throw new PnpmError('PATH_NOT_FOUND', `Path not found: ${patchPath}`)
  }

  const stat = await fs.promises.stat(patchPath)

  if (stat.isDirectory()) {
    const patchFiles = await validateAndGetPatchFiles(patchPath)
    const convertedPaths = await Promise.all(patchFiles.map(convertContentAndFileName))
    return convertedPaths.filter(Boolean).map(convertedPathToPatchedDependencyKeyValue)
  } else {
    return convertSinglePatchFile(patchPath)
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return rcOptionsTypes()
}

export function help (): string {
  return renderHelp({
    description: 'Convert patch files from patch-package format to pnpm format',
    url: docsUrl('patch-convert'),
    usages: [
      'pnpm patch-convert [patchesDir]',
      'pnpm patch-convert [patchFile]',
    ],
  })
}

export type PatchConvertCommandOptions = install.InstallCommandOptions & Pick<Config, 'dir' | 'lockfileDir' | 'patchesDir' | 'rootProjectManifest' | 'patchedDependencies'>
function resolvePatchesPath (opts: PatchConvertCommandOptions, params: string[]): string {
  const basePath = opts.dir ?? process.cwd()
  const patchPath = params[0] ?? opts.patchesDir ?? './patches'
  return path.join(basePath, patchPath).replace(/\\/g, '/')
}

export async function handler (
  opts: PatchConvertCommandOptions,
  params: string[]
): Promise<void> {
  const patchesPath = resolvePatchesPath(opts, params)
  const convertedFiles = await convertPatchFile(patchesPath)

  if (convertedFiles.length === 0) {
    return
  }

  const patchedDependencies = { ...opts.patchedDependencies }
  for (const [key, value] of convertedFiles) {
    patchedDependencies[key] = value
  }

  await updatePatchedDependencies(patchedDependencies, {
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })

  await install.handler(opts)
}

export const commandNames = ['patch-convert']
