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
    'style/object-property-newline': ['error', {allowAllPropertiesOnSameLine: true}],
    'style/operator-linebreak': ['error', 'after'],
    'style/quote-props': ['error', 'as-needed', {unnecessary: true}],
  },
}, {
  files: ['**/*.vue'],
  rules: {
    'vue/first-attribute-linebreak': ['error', {singleline: 'beside', multiline: 'beside'}],
    'vue/html-closing-bracket-newline': ['error', {singleline: 'never', multiline: 'never'}],
    'vue/html-indent': ['error', 2, {attribute: 2, alignAttributesVertically: false}],
    'vue/max-attributes-per-line': ['error', {singleline: 5, multiline: 5}],
    'vue/multiline-html-element-content-newline': 'warn',
    'vue/singleline-html-element-content-newline': 'warn',
  },
}, {
  files: ['**/*.md'],
  rules: {
    'style/max-len': 'off',
  },
}, {
  files: ['**/*.json'],
  rules: {
    'style/max-len': 'off',
  },
});
