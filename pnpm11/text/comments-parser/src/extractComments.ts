import { parseString, stripComments } from 'strip-comments-strings'

import type { CommentSpecifier } from './CommentSpecifier.js'

interface ExtractedComments {
  text: string
  comments: CommentSpecifier[] | undefined
  hasFinalNewline: boolean
}

interface RawComment {
  type: string
  content: string
  index: number
  indexEnd: number
}

export function extractComments (text: string): ExtractedComments {
  const hasFinalNewline = text.endsWith('\n')
  const preparedText = hasFinalNewline ? text : text + '\n'
  const { comments: rawComments } = parseString(preparedText)
  const fullStripped = stripComments(preparedText)
  const stripped = hasFinalNewline ? fullStripped : fullStripped.slice(0, -1)
  const comments = collectComments(rawComments, stripped)

  return {
    text: stripped,
    comments: comments.length ? comments : undefined,
    hasFinalNewline,
  }
}

function collectComments (rawComments: RawComment[], stripped: string): CommentSpecifier[] {
  const comments: CommentSpecifier[] = []
  let offset = 0
  for (const rawComment of rawComments) {
    const commentIndex = rawComment.index - offset
    comments.push(buildCommentSpecifier(rawComment, stripped, commentIndex))
    offset += rawComment.indexEnd - rawComment.index
  }
  return comments
}

function resolvePreambleLineNumber (priorLines: string[]): { lineNumber: number, after?: string } {
  const count = priorLines.length
  if (count === 1) {
    return {
      lineNumber: priorLines[0].trim().length === 0 ? 0 : 1,
    }
  }
  const startsWithEmpty = priorLines[0].trim().length === 0
  return {
    lineNumber: startsWithEmpty ? count - 1 : count,
    after: priorLines[count - 2],
  }
}

function resolveLineBounds (stripped: string, lineStart: number): { lineEnd: number, before?: string } {
  const searchStart = lineStart === 0 ? 0 : lineStart + 1
  const foundLineEnd = stripped.indexOf('\n', searchStart)
  const lineEnd = foundLineEnd < 0 ? stripped.length : foundLineEnd
  const nextLineEnd = stripped.indexOf('\n', lineEnd + 1)
  return {
    lineEnd,
    before: nextLineEnd >= 0 ? stripped.slice(lineEnd, nextLineEnd) : undefined,
  }
}

function buildCommentSpecifier (comment: RawComment, stripped: string, commentIndex: number): CommentSpecifier {
  const preamble = stripped.slice(0, commentIndex)
  const lineStart = Math.max(preamble.lastIndexOf('\n'), 0)
  const { lineNumber, after } = resolvePreambleLineNumber(preamble.split('\n'))
  const { lineEnd, before } = resolveLineBounds(stripped, lineStart)
  const whitespaceMatch = stripped.slice(lineStart, commentIndex).match(/^\s*/)

  const specifier: CommentSpecifier = {
    type: comment.type,
    content: comment.content,
    lineNumber,
    on: stripped.slice(lineStart, lineEnd),
    whitespace: whitespaceMatch ? whitespaceMatch[0] : '',
  }
  if (after !== undefined) specifier.after = after
  if (before !== undefined) specifier.before = before
  return specifier
}

