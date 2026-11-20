import type { CommentSpecifier } from './CommentSpecifier.js'

const CANONICALIZER = /[\s'"]/g

function canonicalize (text: string): string {
  return text.replace(CANONICALIZER, '')
}

export function insertComments (json: string, comments: CommentSpecifier[]): string {
  const jsonLines = json.split('\n')
  const index = buildLineIndex(jsonLines)
  const jsonPrefix: Record<string, string> = {}

  for (const comment of comments) {
    placeComment(comment, jsonLines, index, jsonPrefix)
  }

  prependPrefixes(jsonLines, jsonPrefix)
  return jsonLines.join('\n')
}

function buildLineIndex (jsonLines: string[]): Record<string, number> {
  const index: Record<string, number> = {}
  for (let lineIndex = 0; lineIndex < jsonLines.length; ++lineIndex) {
    const key = canonicalize(jsonLines[lineIndex])
    if (key in index) {
      index[key] = -1
    } else {
      index[key] = lineIndex
    }
  }
  return index
}

function placeComment (
  comment: CommentSpecifier,
  jsonLines: string[],
  index: Record<string, number>,
  jsonPrefix: Record<string, string>
): void {
  if (tryPlaceOnLine(comment, jsonLines, index)) return
  if (tryPlaceAtEnd(comment, jsonLines)) return
  if (tryPlaceBefore(comment, index, jsonPrefix)) return
  if (tryPlaceAfter(comment, jsonLines, index)) return
  placeByLineNumber(comment, jsonLines)
}

function tryPlaceOnLine (
  comment: CommentSpecifier,
  jsonLines: string[],
  index: Record<string, number>
): boolean {
  const key = canonicalize(comment.on)
  if (key && index[key] !== undefined && index[key] >= 0) {
    jsonLines[index[key]] += ' ' + comment.content
    return true
  }
  return false
}

function tryPlaceAtEnd (comment: CommentSpecifier, jsonLines: string[]): boolean {
  if (comment.before === undefined) {
    jsonLines[jsonLines.length - 1] += comment.whitespace + comment.content
    return true
  }
  return false
}

function tryPlaceBefore (
  comment: CommentSpecifier,
  index: Record<string, number>,
  jsonPrefix: Record<string, string>
): boolean {
  let location = comment.lineNumber === 0 ? 0 : -1
  if (location < 0) {
    const key = canonicalize(comment.before!)
    if (key && index[key] !== undefined) {
      location = index[key]
    }
  }
  if (location < 0) return false

  if (jsonPrefix[location]) {
    jsonPrefix[location] += ' ' + comment.content
  } else {
    const inlineWhitespace = comment.whitespace[0] === '\n'
      ? comment.whitespace.slice(1)
      : comment.whitespace
    jsonPrefix[location] = inlineWhitespace + comment.content
  }
  return true
}

function tryPlaceAfter (
  comment: CommentSpecifier,
  jsonLines: string[],
  index: Record<string, number>
): boolean {
  if (!comment.after) return false
  const key = canonicalize(comment.after)
  if (key && index[key] !== undefined && index[key] >= 0) {
    jsonLines[index[key]] += comment.whitespace + comment.content
    return true
  }
  return false
}

function placeByLineNumber (comment: CommentSpecifier, jsonLines: string[]): void {
  let location = comment.lineNumber - 1
  let separator = ' '
  if (location >= jsonLines.length) {
    location = jsonLines.length - 1
    separator = '\n'
  }
  jsonLines[location] += separator + comment.content +
    ' /* [comment possibly relocated by pnpm] */'
}

function prependPrefixes (jsonLines: string[], jsonPrefix: Record<string, string>): void {
  for (let lineIndex = 0; lineIndex < jsonLines.length; ++lineIndex) {
    if (jsonPrefix[lineIndex]) {
      jsonLines[lineIndex] = jsonPrefix[lineIndex] + '\n' + jsonLines[lineIndex]
    }
  }
}

