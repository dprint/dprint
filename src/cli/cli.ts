#!/usr/bin/env node
import { runCli } from "./runCli";
import { CliEnvironment } from "./environment";

runCli(process.argv.slice(2), new CliEnvironment());
