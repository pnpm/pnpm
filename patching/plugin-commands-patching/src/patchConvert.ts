import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import path from 'path'
import fs from 'fs'
import { updatePatchedDependencies } from './updatePatchedDependencies.js'
import { install } from '@pnpm/plugin-commands-installation'
import renderHelp from 'render-help'
import pick from 'ramda/src/pick'
import { docsUrl } from '@pnpm/cli-utils'

function processFolder(folderPath: string): string[] {
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

async function convertContentAndFileName(patchFilePath: string): Promise<string> {
  const patchContent = await fs.promises.readFile(patchFilePath, 'utf8')
  const shouldBeConverted = patchContent.includes('diff --git a/node_modules/') ||
    patchContent.includes('--- a/node_modules/') ||
    patchContent.includes('+++ b/node_modules/')

  if (!shouldBeConverted) {
    return ''
  }
  const replaceStr = `/node_modules/${path.basename(patchFilePath).split('+').slice(0, -1).join('/')}`.replace(/\//g, '/')
  const convertedContent = patchContent.replace(new RegExp(replaceStr, 'g'), '')
  const outputPath = convertPatchNameToPnpmFormat(patchFilePath)

  await fs.promises.writeFile(outputPath, convertedContent, 'utf8')
  await fs.promises.unlink(patchFilePath)
  return path.basename(outputPath)
}

function convertPatchNameToPnpmFormat(patchFileName: string): string {
  const info = patchFileName.split('+')
  const version = info.pop()
  return info.join('__') + '@' + version
}

async function singlePatchFileConvert(patchFilePath: string): Promise<string[]> {
  if (!fs.existsSync(patchFilePath)) {
    throw new PnpmError('FILE_NOT_FOUND', `Patch file not found: ${patchFilePath}`)
  }
  if (!patchFilePath.endsWith('.patch')) {
    throw new PnpmError('INVALID_PATCH_FILE', `File must have .patch extension: ${patchFilePath}`)
  }
  if (!patchFilePath.includes('+')) {
    throw new PnpmError('INVALID_PATCH_FILE_NAME', `Should be convert patch file must include '+': ${patchFilePath}`)
  }
  const covertName = convertPatchNameToPnpmFormat(patchFilePath)
  const allPatches = await fs.promises.readdir(path.dirname(patchFilePath))
  if (allPatches.includes(covertName)) {
    throw new PnpmError('PATCH_ALREADY_EXISTS', `Converted patch file already exists: ${covertName}`)
  }
  return [ await convertContentAndFileName(patchFilePath)]
}

async function convertPatchFile(patchPath: string): Promise<string[]> {
  const stat = await fs.promises.stat(patchPath)
  let patchFiles: string[]

  if (stat.isDirectory()) {
    patchFiles = processFolder(patchPath)
    return (await Promise.all(patchFiles.map(convertContentAndFileName))).filter(Boolean)
  } else {
    if (!fs.existsSync(patchPath)) {
      throw new PnpmError('FILE_NOT_FOUND', `Patch file not found: ${patchPath}`)
    }
    if (!patchPath.endsWith('.patch')) {
      throw new PnpmError('INVALID_PATCH_FILE', `File must have .patch extension: ${patchPath}`)
    }
    if (!patchPath.includes('+')) {
      throw new PnpmError('INVALID_PATCH_FILE_NAME', `Should be convert patch file must include '+': ${patchPath}`)
    }
    const covertName = convertPatchNameToPnpmFormat(patchPath)
    const allPatches = await fs.promises.readdir(path.dirname(patchPath))
    if (allPatches.includes(covertName)) {
      throw new PnpmError('PATCH_ALREADY_EXISTS', `Converted patch file already exists: ${covertName}`)
    }
    return (await singlePatchFileConvert(patchPath)).filter(Boolean)
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
export async function handler(
  opts: PatchConvertCommandOptions,
  params: string[]
): Promise<void> {
  const patchesPath = path.join(params[0] || opts.patchesDir || './patches')

  const pkgConvertedFiles = await convertPatchFile(patchesPath)
  let shouldBeUpdated = false
  const patchedDependencies = opts.patchedDependencies ?? {}
  if (Array.isArray(pkgConvertedFiles) && pkgConvertedFiles.length) {
    pkgConvertedFiles.forEach((file) => {
      const k = file.replace(/\.patch$/, '').replace(/__/g, '/')
      const v = path.join(patchesPath, file).replace(/\\/g, '/')
      patchedDependencies[k] = v
      shouldBeUpdated = true
    })
  }
  if (!shouldBeUpdated) {
    return
  }
  await updatePatchedDependencies(patchedDependencies, {
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })

  await install.handler(opts)
}

export const commandNames = ['patch-convert']
