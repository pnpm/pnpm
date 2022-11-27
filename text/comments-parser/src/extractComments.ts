import { parseString, stripComments } from 'strip-comments-strings'
import { CommentSpecifier } from './CommentSpecifier'

export function extractComments (text: string) {
  const hasFinalNewline = text.endsWith('\n')
  if (!hasFinalNewline) {
    /* For the sake of the comment parser, which otherwise loses the
     * final character of a final comment
     */
    text += '\n'
  }
  const { comments: rawComments } = parseString(text)
  const comments: CommentSpecifier[] = []
  let stripped = stripComments(text)
  if (!hasFinalNewline) {
    stripped = stripped.slice(0, -1)
  }
  let offset = 0 // accumulates difference of indices from text to stripped
  for (const comment of rawComments) {
    /* Extract much more context for the comment needed to restore it later */
    // Unfortunately, JavaScript lastIndexOf does not have an end parameter:
    const preamble: string = stripped.slice(0, comment.index - offset)
    const lineStart = Math.max(preamble.lastIndexOf('\n'), 0)
    const priorLines = preamble.split('\n')
    let lineNumber = priorLines.length
    let after = ''
    let hasAfter = false
    if (lineNumber === 1) {
      if (preamble.trim().length === 0) {
        lineNumber = 0
      }
    } else {
      after = priorLines[lineNumber - 2]
      hasAfter = true
      if (priorLines[0].trim().length === 0) {
        /* JSON5.stringify will not have a whitespace-only line at the start */
        lineNumber -= 1
      }
    }
    let lineEnd = stripped.indexOf(
      '\n', (lineStart === 0) ? 0 : lineStart + 1)
    if (lineEnd < 0) {
      lineEnd = stripped.length
    }
    const whitespaceMatch = stripped
      .slice(lineStart, comment.index - offset)
      .match(/^\s*/)

    const newComment: CommentSpecifier = {
      type: comment.type,
      content: comment.content,
      lineNumber,
      on: stripped.slice(lineStart, lineEnd),
      whitespace: whitespaceMatch ? whitespaceMatch[0] : '',
    }

    if (hasAfter) {
      newComment.after = after
    }
    const nextLineEnd = stripped.indexOf('\n', lineEnd + 1)
    if (nextLineEnd >= 0) {
      newComment.before = stripped.slice(lineEnd, nextLineEnd)
    }
    comments.push(newComment)
    offset += comment.indexEnd - comment.index
  }
  return {
    text: stripped,
    comments: comments.length ? comments : undefined,
    hasFinalNewline,
  }
}
