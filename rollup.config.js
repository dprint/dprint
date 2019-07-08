import typescript from 'rollup-plugin-typescript2';
import obfuscatorPlugin from 'rollup-plugin-javascript-obfuscator';

export default {
  input: './src/index.ts',
  output: {
    file: './dist/dprint.js',
    format: 'cjs'
  },
  plugins: [
    typescript({
        typescript: require("ttypescript"),
    }),
    obfuscatorPlugin()
  ]
};
