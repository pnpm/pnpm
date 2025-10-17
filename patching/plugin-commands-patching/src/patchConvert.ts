import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import path from 'path'
import fs from 'fs'
import { updatePatchedDependencies } from './updatePatchedDependencies.js'
import { install } from '@pnpm/plugin-commands-installation'
import renderHelp from 'render-help'
import pick from 'ramda/src/pick'
import { docsUrl } from '@pnpm/cli-utils'

function processFolder (folderPath: string): string[] {
  if (!fs.existsSync(folderPath)) {
    throw new PnpmError('FOLDER_NOT_FOUND', `Folder not found: ${folderPath}`)
  }

  const stat = fs.statSync(folderPath)
  if (!stat.isDirectory()) {
    throw new PnpmError('NOT_A_DIRECTORY', `Path is not a directory: ${folderPath}`)
  }

  const files = fs.readdirSync(folderPath)
  const patchFiles = files.filter(file => file.endsWith('.patch'))

  if (patchFiles.length === 0) {
    throw new PnpmError('NO_PATCH_FILES', `No patch files found in directory: ${folderPath}`)
  }

  return patchFiles.map(file => path.join(folderPath, file))
}

async function convertContentAndFileName (patchFilePath: string): Promise<string> {
  const patchContent = await fs.promises.readFile(patchFilePath, 'utf8')
  const shouldBeConverted = patchContent.includes('diff --git a/node_modules/') ||
    patchContent.includes('--- a/node_modules/') ||
    patchContent.includes('+++ b/node_modules/')

  if (!shouldBeConverted) {
    return ''
  }
  const replaceStr = `/node_modules/${path.basename(patchFilePath).split('+').slice(0, -1).join('/')}`.replace(/\\/g, '/')
  const convertedContent = patchContent.replace(new RegExp(replaceStr, 'g'), '')
  const outputPath = convertPatchNameToPnpmFormat(patchFilePath)

  await fs.promises.writeFile(outputPath, convertedContent, 'utf8')
  await fs.promises.unlink(patchFilePath)
  return outputPath
}

function convertPatchNameToPnpmFormat (patchFileName: string): string {
  const info = patchFileName.split('+')
  const version = info.pop()
  return info.join('__') + '@' + version
}

function convertedPathToPatchedDependencyKeyValue (convertedPath: string): [string, string] {
  const baseName = path.basename(convertedPath)
  const patchesDir = convertedPath.replace(/\\/g, '/').replace(baseName, '').split('/').filter(Boolean).pop()!
  const key = baseName.replace(/\.patch$/, '').replace(/__/g, '/')
  return [key, patchesDir + '/' + baseName]
}

async function singlePatchFileConvert (patchFilePath: string): Promise<Array<[string, string]>> {
  if (!fs.existsSync(patchFilePath)) {
    throw new PnpmError('FILE_NOT_FOUND', `Patch file not found: ${patchFilePath}`)
  }
  if (!patchFilePath.endsWith('.patch')) {
    throw new PnpmError('INVALID_PATCH_FILE', `File must have .patch extension: ${patchFilePath}`)
  }
  if (!patchFilePath.includes('+')) {
    throw new PnpmError('INVALID_PATCH_FILE_NAME', `Invalid patch file name: expected '+' in the file name (e.g. pkg+1.0.0.patch): ${patchFilePath}`)
  }
  const covertName = convertPatchNameToPnpmFormat(path.basename(patchFilePath))
  const outputPath = path.join(path.dirname(patchFilePath), covertName)
  if (fs.existsSync(outputPath)) {
    throw new PnpmError('PATCH_ALREADY_EXISTS', `Converted patch file already exists: ${covertName}`)
  }
  const output = await convertContentAndFileName(patchFilePath)
  return output ? [convertedPathToPatchedDependencyKeyValue(output)] : []
}

async function convertPatchFile (patchPath: string): Promise<Array<[string, string]>> {
  if (!fs.existsSync(patchPath)) {
    throw new PnpmError('PATH_NOT_FOUND', `Path not found: ${patchPath}`)
  }
  const stat = await fs.promises.stat(patchPath)
  let patchFiles: string[]

  if (stat.isDirectory()) {
    patchFiles = processFolder(patchPath)
    return (await Promise.all(patchFiles.map(convertContentAndFileName))).filter(Boolean).map(convertedPathToPatchedDependencyKeyValue)
  } else {
    return singlePatchFileConvert(patchPath)
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return { ...rcOptionsTypes() }
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
export async function handler (
  opts: PatchConvertCommandOptions,
  params: string[]
): Promise<void> {
  const patchesPath = path.join(opts.dir ?? process.cwd(), params[0] ?? opts.patchesDir ?? './patches').replace(/\\/g, '/')
  const pkgConvertedFiles = await convertPatchFile(patchesPath)
  if (!pkgConvertedFiles.length) {
    return
  }
  const patchedDependencies = opts.patchedDependencies ?? {}
  if (Array.isArray(pkgConvertedFiles) && pkgConvertedFiles.length) {
    pkgConvertedFiles.forEach(([key, value]) => {
      patchedDependencies[key] = value
    })
  }
  await updatePatchedDependencies(patchedDependencies, {
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })

  await install.handler(opts)
}

export const commandNames = ['patch-convert']
