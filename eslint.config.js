import antfu from '@antfu/eslint-config';

export default antfu({
  vue: true,
  ignores: [
    'app/modules/backend/**',
    '**/generated/**',
    'stfc-mod/**',
  ],
  stylistic: {
    semi: true,
  },
}, {
  rules: {
    curly: ['error', 'all'],
    'regexp/strict': 'off',
    'style/block-spacing': ['error', 'never'],
    'style/brace-style': ['error', '1tbs'],
    'style/max-len': ['warn', {code: 120}],
    'style/object-curly-spacing': ['error', 'never'],
    'style/operator-linebreak': ['error', 'after'],
    'style/quote-props': ['error', 'as-needed', {unnecessary: true}],
  },
}, {
  files: ['**/*.vue'],
  rules: {
    'vue/html-indent': ['error', 2, {attribute: 2, alignAttributesVertically: false}],
    'vue/first-attribute-linebreak': ['error', {singleline: 'beside', multiline: 'beside'}],
    'vue/html-closing-bracket-newline': ['error', {singleline: 'never', multiline: 'never'}],
    'vue/max-attributes-per-line': ['error', {singleline: 10, multiline: 10}],
  },
}, {
  files: ['**/*.md'],
  rules: {
    'style/no-trailing-spaces': 'off',
    'style/max-len': 'off',
  },
}, {
  files: ['**/*.json'],
  rules: {
    'style/max-len': 'off',
  },
});
