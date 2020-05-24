## JSON/JSONC Code Formatter

### Install and Setup

Install plugin via:

```bash
yarn add --dev dprint-plugin-jsonc
# or
npm install --save-dev dprint-plugin-jsonc
```

Then add it to the configuration in *dprint.config.js*:

```js
// @ts-check
const { JsoncPlugin } = require("dprint-plugin-jsonc");

/** @type { import("dprint").Configuration } */
module.exports.config = {
    projectType: "openSource",
    plugins: [
        new JsoncPlugin({
            // specify config here
            indentWidth: 2,
        }),
    ],
};
```

Links:

* [Type Declarations](https://github.com/dprint/dprint/blob/master/packages/dprint-plugin-jsonc/lib/dprint-plugin-jsonc.d.ts)


### Configuration

There is currently no JSONC specific configuration beyond the global configuration (ex. `lineWidth`, `indentWidth`, etc.).
