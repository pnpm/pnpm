import type { Catalogs } from '@pnpm/catalogs.types'
import { parsePkgAndParentSelector } from '@pnpm/config.parse-overrides'
import {
  isDriveLetterPrefix,
  isHostFilesystemSpecifier,
  isNonLocalFilesystemSubdepSpec,
  stripPackageNameFromLocalSpec,
} from '@pnpm/resolving.local-resolver'
import type { PackageExtension } from '@pnpm/types'
import normalize from 'normalize-path'

import {
  type AuthorizedLocalEdge,
  isNonRelativeTarget,
  type LocalHookInputs,
  normalizeRelDir,
} from './verifyLockfileLocalPolicy.js'
import { canonicalSlashPath, decodeFileUrlOrSlashPath } from './verifyLockfileShape.js'

export function collectAuthorizedEdges (inputs: LocalHookInputs): AuthorizedLocalEdge[] {
  const edges: AuthorizedLocalEdge[] = []
  const overridesBase = inputs.overridesRootDir ?? inputs.lockfileDir
  for (const [selector, rawSpec] of Object.entries(inputs.overrides ?? {})) {
    const spec = derefCatalogSpec(inputs.catalogs, selector, rawSpec)
    const edge = parseOverrideEdge(selector, spec, overridesBase, inputs.lockfileDir)
    if (edge != null) edges.push(edge)
  }
  for (const [parentSelector, ext] of Object.entries(inputs.packageExtensions ?? {})) {
    for (const depMap of [ext.dependencies, ext.optionalDependencies, ext.peerDependencies]) {
      pushExtensionEdges(parentSelector, depMap, inputs, edges)
    }
  }
  return edges
}

function pushExtensionEdges (
  parentSelector: string,
  depMap: Record<string, string> | undefined,
  inputs: LocalHookInputs,
  out: AuthorizedLocalEdge[]
): void {
  if (depMap == null) return
  for (const [alias, rawSpec] of Object.entries(depMap)) {
    const spec = derefCatalogSpec(inputs.catalogs, alias, rawSpec)
    const edge = parseExtensionEdge(parentSelector, alias, spec, inputs.lockfileDir)
    if (edge != null) out.push(edge)
  }
}

export function derefCatalogSpec (catalogs: Catalogs | undefined, selectorOrAlias: string, spec: string): string {
  if (!spec.startsWith('catalog:') || catalogs == null) return spec
  const catalogName = spec.slice('catalog:'.length) || 'default'
  let targetName = selectorOrAlias
  try {
    targetName = parsePkgAndParentSelector(selectorOrAlias).targetPkg.name
  } catch {
    // Keep raw key on unparseable selector
  }
  return catalogs[catalogName]?.[targetName] ?? spec
}

function parseOverrideEdge (
  selector: string,
  spec: string,
  baseDir: string | undefined,
  lockfileDir: string | undefined
): AuthorizedLocalEdge | undefined {
  const targets = buildSpecTargets(spec, baseDir, lockfileDir)
  if (targets == null) return undefined
  if (selector === '*') {
    return { childAlias: '*', targets }
  }
  try {
    const parsed = parsePkgAndParentSelector(selector)
    return {
      parentName: parsed.parentPkg?.name,
      parentRange: parsed.parentPkg?.bareSpecifier,
      childAlias: parsed.targetPkg.name,
      childRange: parsed.targetPkg.bareSpecifier,
      targets,
    }
  } catch {
    return undefined
  }
}

function parseExtensionEdge (
  parentSelector: string,
  childAlias: string,
  spec: string,
  lockfileDir: string | undefined
): AuthorizedLocalEdge | undefined {
  const targets = buildSpecTargets(spec, lockfileDir, lockfileDir)
  if (targets == null) return undefined
  if (parentSelector === '*') {
    return { childAlias, targets }
  }
  try {
    const parsed = parsePkgAndParentSelector(parentSelector)
    return {
      parentName: parsed.targetPkg.name,
      parentRange: parsed.targetPkg.bareSpecifier,
      childAlias,
      targets,
    }
  } catch {
    return undefined
  }
}

export function buildSpecTargets (
  spec: string,
  baseDir: string | undefined,
  lockfileDir: string | undefined
): string[] | undefined {
  if (!isLocalAuthSpec(spec)) return undefined
  const rawPath = extractSpecLocalPath(spec)
  if (rawPath == null) return undefined
  const decoded = rawPath.startsWith('//') ? decodeFileUrlOrSlashPath(rawPath) : rawPath.replace(/\\/g, '/')
  const isAbs = decoded.startsWith('/') || isDriveLetterPrefix(decoded)
  if (!isAbs && !isNonRelativeTarget(decoded)) {
    return rebaseRelativeTarget(decoded, baseDir, lockfileDir)
  }
  return buildAbsoluteTargets(decoded, isAbs ? lockfileDir : undefined)
}

function isLocalAuthSpec (spec: string): boolean {
  return isNonLocalFilesystemSubdepSpec(spec, () => false) ||
    isHostFilesystemSpecifier(spec, '.', () => false)
}

function buildAbsoluteTargets (slashPath: string, lockfileDir: string | undefined): string[] {
  const primary = slashPath.startsWith('/') || isDriveLetterPrefix(slashPath)
    ? canonicalSlashPath(slashPath)
    : normalizeRelDir(slashPath)
  const targets = [primary]
  if (!lockfileDir) return targets
  const rel = relativePosixPath(lockfileDir, slashPath)
  if (rel != null) {
    const normRel = normalizeRelDir(rel)
    if (!targets.includes(normRel)) targets.push(normRel)
  }
  return targets
}

function rebaseRelativeTarget (
  slashRel: string,
  baseDir: string | undefined,
  lockfileDir: string | undefined
): string[] {
  if (baseDir && lockfileDir && baseDir !== lockfileDir) {
    const joined = `${baseDir.replace(/\\/g, '/')}/${slashRel}`
    const rel = relativePosixPath(lockfileDir, joined)
    return [rel != null ? normalizeRelDir(rel) : canonicalSlashPath(joined)]
  }
  return [normalizeRelDir(slashRel)]
}

interface VolumeRoot {
  kind: 'posix' | 'drive' | 'unc'
  key: string
  minSegments: number
}

export function relativePosixPath (baseDir: string, targetPath: string): string | undefined {
  const baseSlash = baseDir.replace(/\\/g, '/')
  const targetSlash = targetPath.replace(/\\/g, '/')
  const baseVol = parseVolumeRoot(baseSlash)
  const targetVol = parseVolumeRoot(targetSlash)
  if (baseVol == null || targetVol == null || baseVol.kind !== targetVol.kind || baseVol.key !== targetVol.key) {
    return undefined
  }
  const baseParts = normalizedSegments(baseSlash, baseVol.minSegments)
  const targetParts = normalizedSegments(targetSlash, targetVol.minSegments)
  if (baseParts == null || targetParts == null) return undefined
  const caseInsensitive = baseVol.kind !== 'posix'
  let common = 0
  while (
    common < baseParts.length &&
    common < targetParts.length &&
    segmentsMatch(baseParts[common], targetParts[common], caseInsensitive)
  ) {
    common += 1
  }
  const out = [
    ...Array.from({ length: baseParts.length - common }, () => '..'),
    ...targetParts.slice(common),
  ]
  return out.length === 0 ? '.' : normalize(out.join('/'))
}

function segmentsMatch (left: string, right: string, caseInsensitive: boolean): boolean {
  return caseInsensitive ? left.toLowerCase() === right.toLowerCase() : left === right
}

function parseVolumeRoot (slashPath: string): VolumeRoot | undefined {
  if (slashPath.startsWith('//') && !slashPath.startsWith('///')) {
    return parseUncVolumeRoot(slashPath.slice(2))
  }
  if (isDriveLetterPrefix(slashPath) && (slashPath.length === 2 || slashPath[2] === '/')) {
    return { kind: 'drive', key: slashPath[0].toUpperCase(), minSegments: 1 }
  }
  return slashPath.startsWith('/') ? { kind: 'posix', key: '/', minSegments: 0 } : undefined
}

function parseUncVolumeRoot (rest: string): VolumeRoot | undefined {
  const slashIdx = rest.indexOf('/')
  if (slashIdx <= 0) return undefined
  const server = rest.slice(0, slashIdx)
  const share = rest.slice(slashIdx + 1).split('/')[0] ?? ''
  if (!isValidUncPart(server) || !isValidUncPart(share)) return undefined
  return { kind: 'unc', key: `${server.toLowerCase()}/${share.toLowerCase()}`, minSegments: 2 }
}

function isValidUncPart (part: string): boolean {
  return part !== '' && part !== '.' && part !== '..'
}

function normalizedSegments (slashPath: string, minSegments: number): string[] | undefined {
  const stack: string[] = []
  for (const segment of slashPath.split('/')) {
    if (!applySegmentToStack(stack, segment, minSegments)) return undefined
  }
  return stack.length >= minSegments ? stack : undefined
}

function applySegmentToStack (stack: string[], segment: string, minSegments: number): boolean {
  if (!segment || segment === '.') return true
  if (segment !== '..') {
    stack.push(segment)
    return true
  }
  if (stack.length <= minSegments) return false
  stack.pop()
  return true
}

export function extractSpecLocalPath (rawSpec: string): string | undefined {
  const peeled = peelNpmSpecifiers(rawSpec)
  if (peeled == null) return undefined
  const keyLocalSpec = stripPackageNameFromLocalSpec(peeled)
  if (keyLocalSpec != null) {
    const localPath = extractProtocolLocalPath(keyLocalSpec)
    return localPath?.startsWith('//') ? stripPeerSuffix(localPath) : localPath
  }
  const unwrapped = unwrapWorkspaceSpec(peeled)
  const localPath = extractProtocolLocalPath(unwrapped)
  if (localPath != null) return localPath
  if (isDriveLetterPrefix(unwrapped) && (unwrapped[2] === '/' || unwrapped[2] === '\\')) {
    return unwrapped
  }
  return unwrapped.includes(':') ? undefined : unwrapped
}

function extractProtocolLocalPath (spec: string): string | undefined {
  if (spec.slice(0, 9).toLowerCase() === 'git+file:') {
    const rest = spec.slice(9)
    return rest.split('#')[0] ?? rest
  }
  for (const prefix of ['file:', 'link:']) {
    if (spec.slice(0, prefix.length).toLowerCase() === prefix) {
      return spec.slice(prefix.length)
    }
  }
  return undefined
}

function stripPeerSuffix (keyPath: string): string {
  return keyPath.endsWith(')') ? keyPath.split('(')[0] : keyPath
}

function peelNpmSpecifiers (rawSpec: string): string | undefined {
  let spec = rawSpec
  for (let depth = 0; depth < 8; depth++) {
    if (spec.slice(0, 4).toLowerCase() !== 'npm:') break
    const rest = spec.slice(4)
    const atIdx = rest.lastIndexOf('@')
    if (atIdx < 1) return undefined
    spec = rest.slice(atIdx + 1)
  }
  return spec
}

function unwrapWorkspaceSpec (spec: string): string {
  if (spec.slice(0, 'workspace:'.length).toLowerCase() !== 'workspace:') return spec
  const rest = spec.slice('workspace:'.length)
  const atIdx = rest.startsWith('.') ? -1 : rest.lastIndexOf('@')
  return atIdx < 1 ? rest : rest.slice(atIdx + 1)
}

export type { PackageExtension }
