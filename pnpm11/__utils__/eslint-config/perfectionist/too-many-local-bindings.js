import { collectPatternNames, describeFunction, functionHeadLoc, trackMeasuredBodies } from './functions.js'

// The TypeScript counterpart of `perfectionist::too_many_local_bindings`.
// Counts the distinct names a function body binds through variable
// declarations, `catch` clauses, and the parameters of the closures it
// contains. Shadowing is free, a name that begins with `_` is not counted, and
// neither are the function's own parameters.
export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Limit the distinct local names a function body binds',
    },
    schema: [{
      type: 'object',
      properties: { maxBindings: { type: 'integer', minimum: 0 } },
      additionalProperties: false,
    }],
    messages: {
      tooMany: '{{name}} binds {{count}} local names, more than the maximum of {{maxBindings}}. Move each step into a function of its own, or group related values into an object.',
    },
  },
  create (context) {
    const maxBindings = context.options[0]?.maxBindings ?? 12
    const { stack, visitor } = trackMeasuredBodies({
      createState: (node) => ({ node, names: new Set() }),
      onExit: (state) => {
        if (state.names.size <= maxBindings) return
        context.report({
          loc: functionHeadLoc(state.node),
          messageId: 'tooMany',
          data: { name: describeFunction(state.node), count: state.names.size, maxBindings },
        })
      },
      onClosure: (state, node) => {
        for (const param of node.params) addNames(state, param)
      },
    })

    function addNames (state, pattern) {
      for (const name of collectPatternNames(pattern)) {
        if (!name.startsWith('_')) state.names.add(name)
      }
    }

    return {
      ...visitor,
      VariableDeclarator (node) {
        if (stack.length > 0) addNames(stack.at(-1), node.id)
      },
      CatchClause (node) {
        if (stack.length > 0 && node.param != null) addNames(stack.at(-1), node.param)
      },
    }
  },
}
