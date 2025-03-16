export default {
  meta: {
		type: "problem",
		docs: {
			description: "no duplicate condition",
			category: "Possible Errors",
		}
	},
	create(context) {
		return {
			IfStatement(node) {
				const conditions = [];
				function extractConditions(conditionNode) {
					if (conditionNode.type === 'LogicalExpression') {
						extractConditions(conditionNode.left);
						extractConditions(conditionNode.right);
					} else {
						conditions.push(context.getSourceCode().getText(conditionNode));
					}
				}

				extractConditions(node.test);
				const duplicateConditions = conditions.filter((condition, index) => conditions.indexOf(condition) !== index);
				if (duplicateConditions.length > 0) {
					context.report({
						node: node.test,
						message: `Duplicate condition(s) found: ${duplicateConditions.join(
              ", "
            )}`,
					});
				}
			}
		}
	}
}