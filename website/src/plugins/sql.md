---
title: SQL Plugin
description: Documentation on the SQL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# SQL Code Formatter

Formats .sql files written for PostgreSQL, MySQL and MariaDB, SQL Server (T-SQL), SQLite, Oracle (PL/SQL), BigQuery and Snowflake, including stored procedures, functions, triggers and the other procedural code in them.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add sql
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"sql"` property to add configuration:

```json
{
  "sql": {
    // sql config goes here
    "dialect": "postgresql"
  },
  "plugins": [
    "npm:@dprint/sql@x.x.x"
  ]
}
```

## Dialects

The `dialect` option says which dialect the files are written in: `generic` (the default), `postgresql`, `mysql`, `sqlserver`, `sqlite`, `oracle`, `bigquery` or `snowflake`. The dialects disagree on how to read some text, such as whether `#` starts a comment (MySQL, BigQuery), names a temporary table (SQL Server) or is an operator (PostgreSQL), and whether a backslash escapes a quote in a string (MySQL, BigQuery, Snowflake).

The `generic` dialect accepts the syntax of all of them where they don't conflict. Set the dialect when a file doesn't format as expected, or formatting reports a string that isn't terminated.

Client commands are kept as they are: `GO` and `sqlcmd` commands for SQL Server, `DELIMITER` for MySQL, `/` and SQL\*Plus commands for Oracle, and psql's meta-commands for PostgreSQL.

## Procedural Code

Stored procedures, functions, triggers and anonymous blocks are formatted along with the statements in them, for PL/pgSQL, MySQL's compound statements, T-SQL (including statements without semicolons), PL/SQL packages and procedures, and the scripting of BigQuery and Snowflake. The body of a PostgreSQL or Snowflake routine in a dollar-quoted string (`$$ .. $$`) is formatted when its language is SQL or PL/pgSQL.

## Ignoring Statements

A statement after a `-- dprint-ignore` comment is left as it is, and a file starting with a `-- dprint-ignore-file` comment isn't formatted.

```sql
-- dprint-ignore
SELECT   a,   b
FROM     t;
```

## Configuration

See [Configuration](/plugins/sql/config)

## Source

See https://github.com/dprint/dprint-plugin-sql
