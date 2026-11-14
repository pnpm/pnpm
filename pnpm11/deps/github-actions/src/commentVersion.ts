import semver from 'semver'

export type ParsedCommentVersion =
  | { kind: 'exact', version: semver.SemVer }
  | { kind: 'majorMinor', major: number, minor: number }
  | { kind: 'major', major: number }

const PARTIAL_VERSION_PATTERN = /^v?(\d+)(?:\.(\d+))?$/

export function parseCommentVersion (input: string): ParsedCommentVersion | undefined {
  const exact = semver.parse(input, { loose: true })
  if (exact != null) return { kind: 'exact', version: exact }
  const match = PARTIAL_VERSION_PATTERN.exec(input)
  if (match == null) return undefined
  const major = Number(match[1])
  if (!Number.isSafeInteger(major)) return undefined
  if (match[2] != null) {
    const minor = Number(match[2])
    return Number.isSafeInteger(minor) ? { kind: 'majorMinor', major, minor } : undefined
  }
  return { kind: 'major', major }
}

export function getCommentVersionToken (comment: string | null | undefined): string | undefined {
  const candidate = comment?.trimStart().split(/\s/, 1)[0]
  return candidate != null && parseCommentVersion(candidate) != null ? candidate : undefined
}

export function shouldReplaceCommentVersion (token: string, current: semver.SemVer): boolean {
  const parsed = parseCommentVersion(token)
  if (parsed == null) return false
  if (token.startsWith('v')) return true
  switch (parsed.kind) {
    case 'exact':
      return true
    case 'majorMinor':
      return current.major === parsed.major && current.minor === parsed.minor
    case 'major':
      return current.major === parsed.major
  }
}
