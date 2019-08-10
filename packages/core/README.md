# @dprint/core

The core functionality for dprint.

* [Api Declarations](lib/dprint-core.d.ts)

Contains:

* Print Items - Plugins use these to declaritively tell the printer how the final file should look like.
* Global Configuration - Global configuration and its result.
* `formatFileText` - Function for formatting the file text.
* Printer (internal) - Accepts an iterable of print items, resolves the conditions and infos, and decides how to lay out the final text.
* Writer (internal) - Used by the printer to write out the final text. This is like an advanced string builder.
