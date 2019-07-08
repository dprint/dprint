#!/usr/bin/env node
import { runCli, CliEnvironment } from "./index";

const environment = new CliEnvironment();

runCli(process.argv.slice(2), environment).catch(err => {
    environment.error(err);
});
