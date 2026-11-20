import { collectPatternNames, isMeasuredFunction } from './functions.js'

// The TypeScript counterparts of perfectionist's `single_letter_*` rules.
// `n` names a count; `f` is also allowed as a parameter, and `i`, `j`, and
// `k` name indices. A closure parameter is also allowed a single letter when
// the closure is a trivial callback: a sort comparator or reducer, or a body
// that only reads a member of the parameter, calls a method on it, or passes
// it to a function as the sole argument.

const LET_BINDING_ALLOWED = new Set(['n'])
const PARAM_ALLOWED = new Set(['n', 'f', 'i', 'j', 'k'])
const TRIVIAL_CALLBACK_METHODS = new Set(['reduce', 'reduceRight', 'sort', 'toSorted'])

const isSingleLetter = (name) => /^[a-z]$/i.test(name)

function createRule ({ description, message, check }) {
  return {
    meta: {
      type: 'suggestion',
      docs: { description },
      schema: [],
      messages: { singleLetter: message },
    },
    create: (context) => check((node, name) => {
      context.report({ node, messageId: 'singleLetter', data: { name } })
    }),
  }
}

function reportPatternNames (report, pattern, allowed) {
  for (const name of collectPatternNames(pattern)) {
    if (isSingleLetter(name) && !allowed.has(name)) report(pattern, name)
  }
}

export const singleLetterLetBinding = createRule({
  description: 'Disallow single-letter variable names',
  message: 'Variable `{{name}}` has a single-letter name. Name it for what it holds.',
  check: (report) => ({
    VariableDeclarator (node) {
      reportPatternNames(report, node.id, LET_BINDING_ALLOWED)
    },
    CatchClause (node) {
      if (node.param != null) reportPatternNames(report, node.param, LET_BINDING_ALLOWED)
    },
  }),
})

export const singleLetterFunctionParam = createRule({
  description: 'Disallow single-letter function and method parameter names',
  message: 'Parameter `{{name}}` has a single-letter name. Name it for what it holds.',
  check: (report) => ({
    'ArrowFunctionExpression, FunctionDeclaration, FunctionExpression' (node) {
      if (!isMeasuredFunction(node)) return
      for (const param of node.params) reportPatternNames(report, param, PARAM_ALLOWED)
    },
  }),
})

export const singleLetterClosureParam = createRule({
  description: 'Disallow single-letter closure parameter names outside trivial callbacks',
  message: 'Closure parameter `{{name}}` has a single-letter name. Name it for what it holds.',
  check: (report) => ({
    'ArrowFunctionExpression, FunctionExpression' (node) {
      if (isMeasuredFunction(node) || isTrivialCallback(node)) return
      for (const param of node.params) reportPatternNames(report, param, PARAM_ALLOWED)
    },
  }),
})

export const singleLetterGeneric = createRule({
  description: 'Disallow single-letter generic type parameter names',
  message: 'Type parameter `{{name}}` has a single-letter name. Name it for the role the type plays.',
  check: (report) => ({
    TSTypeParameter (node) {
      if (isSingleLetter(node.name.name)) report(node, node.name.name)
    },
  }),
})

function isTrivialCallback (node) {
  const expression = singleExpressionBody(node)
  if (expression == null) return false
  if (isArgumentOfTrivialCallbackMethod(node)) return true
  const params = new Set(node.params.flatMap((param) => collectPatternNames(param)))
  return isTrivialWrapper(peel(expression), params)
}

function singleExpressionBody (node) {
  if (node.body.type !== 'BlockStatement') return node.body
  const [statement] = node.body.body
  if (node.body.body.length !== 1) return null
  if (statement.type === 'ReturnStatement') return statement.argument
  if (statement.type === 'ExpressionStatement') return statement.expression
  return null
}

function isArgumentOfTrivialCallbackMethod (node) {
  const { parent } = node
  return parent.type === 'CallExpression' &&
    parent.arguments[0] === node &&
    parent.callee.type === 'MemberExpression' &&
    parent.callee.property.type === 'Identifier' &&
    TRIVIAL_CALLBACK_METHODS.has(parent.callee.property.name)
}

function isTrivialWrapper (expression, params) {
  switch (expression?.type) {
  case 'MemberExpression':
    return isRootedAtParam(expression, params)
  case 'CallExpression':
    if (expression.callee.type === 'MemberExpression') return isRootedAtParam(expression.callee, params)
    return expression.arguments.length === 1 && isParam(peel(expression.arguments[0]), params)
  default:
    return false
  }
}

function isRootedAtParam (node, params) {
  let object = peel(node)
  while (object.type === 'MemberExpression' || object.type === 'CallExpression') {
    object = peel(object.type === 'MemberExpression' ? object.object : object.callee)
  }
  return isParam(object, params)
}

function isParam (node, params) {
  return node.type === 'Identifier' && params.has(node.name)
}

function peel (node) {
  while (node?.type === 'TSNonNullExpression' || node?.type === 'ChainExpression' || node?.type === 'AwaitExpression') {
    node = node.type === 'AwaitExpression' ? node.argument : node.expression
  }
  return node
}
