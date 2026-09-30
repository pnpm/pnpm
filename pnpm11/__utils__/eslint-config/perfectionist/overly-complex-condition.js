import { isFunction } from './functions.js'

const BOOLEAN_OPERATORS = new Set(['&&', '||'])

// The TypeScript counterpart of `perfectionist::overly_complex_condition`.
// Counts the `&&` and `||` operators in the condition of an `if`, a loop, or
// a ternary. A function inside the condition is a scope of its own.
export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Limit the boolean operators in a condition',
    },
    schema: [{
      type: 'object',
      properties: { maxOperators: { type: 'integer', minimum: 0 } },
      additionalProperties: false,
    }],
    messages: {
      tooComplex: 'This condition has {{count}} boolean operators, more than the maximum of {{maxOperators}}. Bind the part that names a concept to a variable.',
    },
  },
  create (context) {
    const maxOperators = context.options[0]?.maxOperators ?? 5

    function check (test) {
      if (test == null) return
      const count = countOperators(test, context.sourceCode.visitorKeys)
      if (count > maxOperators) {
        context.report({ node: test, messageId: 'tooComplex', data: { count, maxOperators } })
      }
    }

    return {
      ':matches(IfStatement, WhileStatement, DoWhileStatement, ForStatement, ConditionalExpression)' (node) {
        check(node.test)
      },
    }
  },
}

function countOperators (node, visitorKeys) {
  if (isFunction(node)) return 0
  let count = node.type === 'LogicalExpression' && BOOLEAN_OPERATORS.has(node.operator) ? 1 : 0
  for (const key of visitorKeys[node.type] ?? []) {
    for (const child of [node[key]].flat()) {
      if (child?.type != null) count += countOperators(child, visitorKeys)
    }
  }
  return count
}
