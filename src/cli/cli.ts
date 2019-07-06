#!/usr/bin/env node
import { runCli } from "./runCli";
import { CliEnvironment } from "./environment";

const environment = new CliEnvironment();

runCli(process.argv.slice(2), environment).catch(err => {
    environment.error(err);
});
