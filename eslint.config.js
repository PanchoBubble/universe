import eslint from '@eslint/js';
import tseslint from 'typescript-eslint';
import react from 'eslint-plugin-react';
import hooksPlugin from 'eslint-plugin-react-hooks';
import stylistic from '@stylistic/eslint-plugin'

import prettierConfig from 'eslint-config-prettier';
import globals from 'globals';

import tsParser from '@typescript-eslint/parser';

export default [
    react.configs.flat.recommended,
    react.configs.flat['jsx-runtime'],
    eslint.configs.recommended,
    ...tseslint.configs.recommended,
    {
        settings: {
            react: {
                version: 'detect',
            },
        },
        languageOptions: {
            globals: { ...globals.browser },
            parser: tsParser,
            parserOptions: { ecmaFeatures: { jsx: true } },
        },
        files: ['src/**/*.{js,ts,jsx,tsx}'],
        ignores: ['./**/*.config.{js,ts}'],
        plugins: { react, 'react-hooks': hooksPlugin, '@stylistic': stylistic },
        rules: {
            ...react.configs.recommended.rules,
            ...hooksPlugin.configs.recommended.rules,
            ...prettierConfig.rules,
            'react/jsx-uses-react': 'off',
            'react/react-in-jsx-scope': 'off',
            'no-console': ['warn', { allow: ['info', 'warn', 'debug', 'error'] }],
            'react/prop-types': 'off',
            '@typescript-eslint/no-explicit-any': 'warn',
            '@typescript-eslint/no-unused-vars': 'warn',
            '@stylistic/indent': ['error', 4],
        },
    },
];
