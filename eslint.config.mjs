import eslintConfig from "@pnpm/eslint-config";
import * as regexpPlugin from "eslint-plugin-regexp";

export default [{
    ignores: ["**/fixtures", "**/__fixtures__", "**/node_modules", "**/lib"],
}, ...eslintConfig, regexpPlugin.configs['flat/recommended']];
