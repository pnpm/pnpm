import { describeFunction, FUNCTION_SELECTOR, functionHeadLoc, isMeasuredFunction } from './functions.js'

// The TypeScript counterpart of `perfectionist::overly_long_function`. A line
// counts when it holds a token of the body, so blank lines and comment-only
// lines are free, and so are the braces that open and close the body. The
// lines of a closure or a nested function count toward the function that
// contains it.
export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Limit the lines of code in a function body',
    },
    schema: [{
      type: 'object',
      properties: { maxLines: { type: 'integer', minimum: 0 } },
      additionalProperties: false,
    }],
    messages: {
      tooLong: '{{name}} has {{count}} lines of code, more than the maximum of {{maxLines}}. Extract the steps into functions named for what each one does.',
    },
  },
  create (context) {
    const maxLines = context.options[0]?.maxLines ?? 40
    const { sourceCode } = context
    return {
      [FUNCTION_SELECTOR] (node) {
        if (!isMeasuredFunction(node)) return
        const count = countCodeLines(sourceCode, node.body)
        if (count <= maxLines) return
        context.report({
          loc: functionHeadLoc(node),
          messageId: 'tooLong',
          data: { name: describeFunction(node), count, maxLines },
        })
      },
    }
  },
}

function countCodeLines (sourceCode, body) {
  let tokens = sourceCode.getTokens(body)
  if (body.type === 'BlockStatement') tokens = tokens.slice(1, -1)
  const lines = new Set()
  for (const token of tokens) {
    for (let line = token.loc.start.line; line <= token.loc.end.line; line++) lines.add(line)
  }
  return lines.size
}
