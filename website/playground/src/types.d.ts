declare module "workerize-loader!*" {
  function createInstance(): Worker;
  export = createInstance;
}
