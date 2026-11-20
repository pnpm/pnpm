// The TypeScript counterpart of `perfectionist::overly_long_method_chain`.
// Counts the method calls in one chain, such as `list.filter(...).map(...)`,
// and reports the chain once, at its outermost call.
export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Limit the method calls in one expression chain',
    },
    schema: [{
      type: 'object',
      properties: { maxCalls: { type: 'integer', minimum: 0 } },
      additionalProperties: false,
    }],
    messages: {
      tooLong: 'This chain has {{count}} method calls, more than the maximum of {{maxCalls}}. Name an intermediate value or extract a helper.',
    },
  },
  create (context) {
    const maxCalls = context.options[0]?.maxCalls ?? 5
    return {
      CallExpression (node) {
        if (isInnerLink(node)) return
        const count = countMethodCalls(node)
        if (count > maxCalls) {
          context.report({ node, messageId: 'tooLong', data: { count, maxCalls } })
        }
      },
    }
  },
}

function unwrap (node) {
  while (node.type === 'TSNonNullExpression' || node.type === 'ChainExpression' || node.type === 'AwaitExpression') {
    node = node.type === 'AwaitExpression' ? node.argument : node.expression
  }
  return node
}

function countMethodCalls (node) {
  let count = 0
  for (let link = unwrap(node); link.type === 'CallExpression' || link.type === 'MemberExpression'; link = unwrap(link.type === 'CallExpression' ? link.callee : link.object)) {
    if (link.type === 'CallExpression' && unwrap(link.callee).type === 'MemberExpression') count++
  }
  return count
}

function isInnerLink (node) {
  let child = node
  let { parent } = node
  while (parent.type === 'TSNonNullExpression' || parent.type === 'ChainExpression' || parent.type === 'AwaitExpression' ||
    (parent.type === 'MemberExpression' && parent.object === child) ||
    (parent.type === 'CallExpression' && parent.callee === child)) {
    if (parent.type === 'CallExpression') return true
    child = parent
    parent = parent.parent
  }
  return false
}
