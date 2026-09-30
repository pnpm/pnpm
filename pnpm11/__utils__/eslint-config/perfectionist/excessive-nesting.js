import { describeFunction, functionHeadLoc, isChainedTernary, isElseIf, trackMeasuredBodies } from './functions.js'

const ALWAYS_NESTING = ':matches(SwitchStatement, TryStatement, ForStatement, ForInStatement, ForOfStatement, WhileStatement, DoWhileStatement)'

// The TypeScript counterpart of `perfectionist::excessive_nesting`. Each
// construct is a level: an `if` (an `else if` stays on the same level), a
// ternary (a ternary in another's `else` position stays on the same level),
// a `switch`, a loop, a `try`, a closure, and a free-standing block. The
// block that is a construct's own body is not a level of its own.
export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Limit how deeply the constructs in a function body nest',
    },
    schema: [{
      type: 'object',
      properties: { maxDepth: { type: 'integer', minimum: 0 } },
      additionalProperties: false,
    }],
    messages: {
      tooDeep: '{{name}} nests constructs {{depth}} levels deep, more than the maximum of {{maxDepth}}. Flatten it with early returns or extract a named helper.',
    },
  },
  create (context) {
    const maxDepth = context.options[0]?.maxDepth ?? 3
    const { stack, visitor } = trackMeasuredBodies({
      createState: (node) => ({ node, depth: 0, maxReached: 0 }),
      onExit: (state) => {
        if (state.maxReached <= maxDepth) return
        context.report({
          loc: functionHeadLoc(state.node),
          messageId: 'tooDeep',
          data: { name: describeFunction(state.node), depth: state.maxReached, maxDepth },
        })
      },
      onClosure: enter,
      onClosureExit: leave,
    })

    function enter (state) {
      state.depth++
      state.maxReached = Math.max(state.maxReached, state.depth)
    }

    function leave (state) {
      state.depth--
    }

    function enterLevel () {
      if (stack.length > 0) enter(stack.at(-1))
    }

    function leaveLevel () {
      if (stack.length > 0) leave(stack.at(-1))
    }

    function unlessContinued (isContinuation, callback) {
      return (node) => {
        if (!isContinuation(node)) callback()
      }
    }

    const isFreeStandingBlock = (node) => node.parent.type === 'BlockStatement'
    return {
      ...visitor,
      IfStatement: unlessContinued(isElseIf, enterLevel),
      'IfStatement:exit': unlessContinued(isElseIf, leaveLevel),
      ConditionalExpression: unlessContinued(isChainedTernary, enterLevel),
      'ConditionalExpression:exit': unlessContinued(isChainedTernary, leaveLevel),
      [ALWAYS_NESTING]: enterLevel,
      [`${ALWAYS_NESTING}:exit`]: leaveLevel,
      BlockStatement: (node) => {
        if (isFreeStandingBlock(node)) enterLevel()
      },
      'BlockStatement:exit': (node) => {
        if (isFreeStandingBlock(node)) leaveLevel()
      },
    }
  },
}
