
import { ESLintUtils } from '@typescript-eslint/utils'
import ts from 'typescript'

const createRule = ESLintUtils.RuleCreator(
    (name) => `https://github.com/pnpm/pnpm/blob/main/tools/eslint-config/${name}`
)

export default createRule({
    name: 'no-object-methods-on-map',
    meta: {
        type: 'problem',
        docs: {
            description: 'Disallow Object.entries/keys/values() on Map/Set objects',
        },
        messages: {
            noObjectMethodsOnMap: 'Object.{{method}}() on a Map/Set always returns empty array. Use Map.prototype.{{method}}() or iterate directly.',
        },
        schema: [],
    },
    defaultOptions: [],
    create(context) {
        const services = ESLintUtils.getParserServices(context)
        const checker = services.program.getTypeChecker()

        return {
            CallExpression(node) {
                if (
                    node.callee.type !== 'MemberExpression' ||
                    node.callee.object.name !== 'Object' ||
                    !['entries', 'keys', 'values'].includes(node.callee.property.name) ||
                    node.arguments.length === 0
                ) {
                    return
                }

                const method = node.callee.property.name
                const arg = node.arguments[0]
                const tsNode = services.esTreeNodeToTSNodeMap.get(arg)
                const type = checker.getTypeAtLocation(tsNode)
                const symbol = type.getSymbol()

                if (symbol && (symbol.name === 'Map' || symbol.name === 'Set')) {
                    context.report({
                        node,
                        messageId: 'noObjectMethodsOnMap',
                        data: { method },
                    })
                }
            },
        }
    },
})
