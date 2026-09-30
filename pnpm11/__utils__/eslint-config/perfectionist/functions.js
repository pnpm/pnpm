// Mirrors how perfectionist splits Rust code into measured bodies: a named
// function or method is measured on its own, while a closure belongs to the
// body that contains it. In TypeScript an arrow function or function
// expression is a closure, unless nothing encloses it, it is a method, or it
// is the callback of a test-framework call, which plays the role of a
// `#[test]` function.

const FUNCTION_TYPES = new Set([
  'ArrowFunctionExpression',
  'FunctionDeclaration',
  'FunctionExpression',
])

const TEST_FRAMEWORK_CALLEES = new Set([
  'afterAll',
  'afterEach',
  'beforeAll',
  'beforeEach',
  'describe',
  'it',
  'test',
])

export const FUNCTION_SELECTOR = 'ArrowFunctionExpression, FunctionDeclaration, FunctionExpression'

export function isFunction (node) {
  return FUNCTION_TYPES.has(node.type)
}

export function isMeasuredFunction (node) {
  if (node.type === 'FunctionDeclaration') return true
  const { parent } = node
  switch (parent.type) {
  case 'MethodDefinition':
  case 'TSAbstractMethodDefinition':
    return true
  case 'Property':
    if (parent.method || parent.kind !== 'init') return true
    break
  case 'PropertyDefinition':
    if (parent.value === node) return true
    break
  case 'CallExpression':
    if (parent.arguments.includes(node) && isTestFrameworkCallee(parent.callee)) return true
    break
  }
  return findEnclosingFunction(node) == null
}

export function findEnclosingFunction (node) {
  for (let ancestor = node.parent; ancestor != null; ancestor = ancestor.parent) {
    if (isFunction(ancestor)) return ancestor
  }
  return null
}

function isTestFrameworkCallee (callee) {
  // Covers `test(...)`, `test.skip(...)`, `describe.each(table)(...)`, and similar.
  let node = callee
  while (node.type === 'MemberExpression' || node.type === 'CallExpression') {
    node = node.type === 'MemberExpression' ? node.object : node.callee
  }
  return node.type === 'Identifier' && TEST_FRAMEWORK_CALLEES.has(node.name)
}

export function functionName (node) {
  if (node.id?.type === 'Identifier') return node.id.name
  const { parent } = node
  if (parent.type === 'VariableDeclarator' && parent.id.type === 'Identifier') return parent.id.name
  if ((parent.type === 'MethodDefinition' || parent.type === 'Property' || parent.type === 'PropertyDefinition') && parent.key.type === 'Identifier') {
    return parent.key.name
  }
  return null
}

export function describeFunction (node) {
  const name = functionName(node)
  return name == null ? 'function' : `function \`${name}\``
}

/** The location of a function's head, so a report does not underline its whole body. */
export function functionHeadLoc (node) {
  return { start: node.loc.start, end: node.body.loc.start }
}

/**
 * Builds the visitor that keeps one state object per measured function body.
 * `onClosure` is called with the current state when a closure opens and
 * `onClosureExit` when it closes, so a rule can treat a closure as a nesting
 * level of the body it belongs to.
 */
export function trackMeasuredBodies ({ createState, onExit, onClosure, onClosureExit }) {
  const stack = []
  return {
    stack,
    visitor: {
      [FUNCTION_SELECTOR] (node) {
        if (isMeasuredFunction(node)) {
          stack.push(createState(node))
        } else if (stack.length > 0) {
          onClosure?.(stack.at(-1), node)
        }
      },
      [`:matches(${FUNCTION_SELECTOR}):exit`] (node) {
        if (isMeasuredFunction(node)) {
          onExit(stack.pop())
        } else if (stack.length > 0) {
          onClosureExit?.(stack.at(-1), node)
        }
      },
    },
  }
}

export function isElseIf (node) {
  return node.parent.type === 'IfStatement' && node.parent.alternate === node
}

export function isChainedTernary (node) {
  return node.parent.type === 'ConditionalExpression' && node.parent.alternate === node
}

export function collectPatternNames (pattern, names = []) {
  switch (pattern?.type) {
  case 'Identifier':
    names.push(pattern.name)
    break
  case 'ObjectPattern':
    for (const property of pattern.properties) {
      collectPatternNames(property.type === 'RestElement' ? property.argument : property.value, names)
    }
    break
  case 'ArrayPattern':
    for (const element of pattern.elements) collectPatternNames(element, names)
    break
  case 'AssignmentPattern':
    collectPatternNames(pattern.left, names)
    break
  case 'RestElement':
    collectPatternNames(pattern.argument, names)
    break
  case 'TSParameterProperty':
    collectPatternNames(pattern.parameter, names)
    break
  }
  return names
}
