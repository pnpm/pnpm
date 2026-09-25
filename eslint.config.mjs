import eslintConfig from "@pnpm/eslint-config";
import * as regexpPlugin from "eslint-plugin-regexp";

export default [
    {
        ignores: ["**/fixtures", "**/__fixtures__", "**/node_modules", "**/lib", ".claude/**", "bench-work-env/**"],
    },
    ...eslintConfig,
    regexpPlugin.configs['flat/recommended'],
    {
        files: ["pnpm11/pnpm/src/**/*.ts"],
        rules: {
            "import-x/no-extraneous-dependencies": "off",
        },
    },
]
