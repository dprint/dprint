# dprint-development

Crate for helping to test Rust dprint plugins.

## Test Specs

This crate provides a helper function for running test specs defined in text files (for the API, see [functions](https://docs.rs/dprint-development/latest/dprint_development/#functions)) in the documentation).

This allows you to write tests in the following format (example with TypeScript):

```
== description goes here ==
const    u    =     2;

[expect]
const u = 2;
```

For a real world example, see [dprint-plugin-typescript/tests](https://github.com/dprint/dprint-plugin-typescript/tree/main/tests).

### Changing File Name

By default, the file name used is the one provided to [`ParseSpecOptions`](https://docs.rs/dprint-development/latest/dprint_development/struct.ParseSpecOptions.html), but you can change the default file name used on a per test spec file basis by adding for example the following to the top of the file:

```
-- file.tsx --
```

### Configuration

To change the configuration, use the following at the top of the file and below the file name if provided:

```
~~ indentWidth: 2, useTabs: true ~~
```

### Test Spec Description Helpers

You may change how all the tests are run by adding certain words to a test description:

- `(only)` - Only runs this test.
- `(skip)` - Skips running this test.
- `(skip-format-twice)` - Skips formatting the output again to ensure it stays the same—only formats once.
- `(trace)` - Only runs this test and outputs the IR graph to an HTML file to view in a web browser. Must be run with `cargo test --features tracing`

For example, adding `(only)` to the description will only run the first test in this example (you'll need to filter using `cargo test` to only run that specific test though):

```
== test 1 (only) ==
const    u    =     2;

[expect]
const u = 2;

== test 2 ==
console.log(  10 )

[expect]
console.log(10);
```

### Only Running Tests In A File

Note the name of the test that corresponds to the current file, and run with `cargo test <name of test>`

### Overwriting Failures

Sometimes a change may cause large test failures (ex. changing default space indentation from 4 spaces to 2 spaces). If you don't want to update all the tests manually, you can specify `fix_failures: true` to `RunSpecsOptions` when calling [`run_specs`](https://docs.rs/dprint-development/latest/dprint_development/fn.run_specs.html).
