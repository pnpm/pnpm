import { describeFunction, functionHeadLoc, functionName, isChainedTernary, isElseIf, trackMeasuredBodies } from './functions.js'

const LOOPS = ':matches(ForStatement, ForInStatement, ForOfStatement, WhileStatement, DoWhileStatement)'
const BOOLEAN_OPERATORS = new Set(['&&', '||'])

// The TypeScript counterpart of `perfectionist::excessive_cognitive_complexity`,
// which scores SonarSource's Cognitive Complexity:
//
// | Construct                                         |     Increment     |
// |---------------------------------------------------|:-----------------:|
// | `if`, ternary, `switch`, loop, `catch`            | 1 + nesting depth |
// | `else if`, `else`, a ternary in `else` position   |         1         |
// | each run of like `&&` / `||` operators            |         1         |
// | each labelled `break` / `continue`                |         1         |
// | each recursive call                               |         1         |
//
// A closure adds nothing itself but deepens the nesting of what it contains.
export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Limit the cognitive complexity of a function body',
    },
    schema: [{
      type: 'object',
      properties: { maxComplexity: { type: 'integer', minimum: 0 } },
      additionalProperties: false,
    }],
    messages: {
      tooComplex: '{{name}} has a cognitive complexity of {{complexity}}, more than the maximum of {{maxComplexity}}. Split it into functions named for what each step does.',
    },
  },
  create (context) {
    const maxComplexity = context.options[0]?.maxComplexity ?? 10
    const { stack, visitor } = trackMeasuredBodies({
      createState: (node) => ({ node, name: functionName(node), nesting: 0, complexity: 0 }),
      onExit: (state) => {
        if (state.complexity <= maxComplexity) return
        context.report({
          loc: functionHeadLoc(state.node),
          messageId: 'tooComplex',
          data: { name: describeFunction(state.node), complexity: state.complexity, maxComplexity },
        })
      },
      onClosure: (state) => {
        state.nesting++
      },
      onClosureExit: (state) => {
        state.nesting--
      },
    })

    function current () {
      return stack.at(-1)
    }

    function addFlat () {
      const state = current()
      if (state) state.complexity++
    }

    function enterStructure () {
      const state = current()
      if (!state) return
      state.complexity += 1 + state.nesting
      state.nesting++
    }

    function leaveStructure () {
      const state = current()
      if (state) state.nesting--
    }

    return {
      ...visitor,
      IfStatement (node) {
        if (isElseIf(node)) {
          addFlat()
        } else {
          enterStructure()
        }
        if (node.alternate != null && node.alternate.type !== 'IfStatement') addFlat()
      },
      'IfStatement:exit' (node) {
        if (!isElseIf(node)) leaveStructure()
      },
      ConditionalExpression (node) {
        if (isChainedTernary(node)) {
          addFlat()
        } else {
          enterStructure()
        }
      },
      'ConditionalExpression:exit' (node) {
        if (!isChainedTernary(node)) leaveStructure()
      },
      [`:matches(SwitchStatement, CatchClause, ${LOOPS})`]: enterStructure,
      [`:matches(SwitchStatement, CatchClause, ${LOOPS}):exit`]: leaveStructure,
      LogicalExpression (node) {
        if (!BOOLEAN_OPERATORS.has(node.operator)) return
        const continuesRun = node.parent.type === 'LogicalExpression' && node.parent.operator === node.operator
        if (!continuesRun) addFlat()
      },
      ':matches(BreakStatement, ContinueStatement)[label]': addFlat,
      CallExpression (node) {
        const state = current()
        if (state?.name != null && node.callee.type === 'Identifier' && node.callee.name === state.name) {
          state.complexity++
        }
      },
    }
  },
}
