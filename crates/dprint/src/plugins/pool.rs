use crate::environment::Environment;
use crate::types::ErrBox;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use super::{Plugin, InitializedPlugin};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

// todo: add a release function that releases a pool and adds any of its created instances into the pool

pub struct PluginPools<TEnvironment : Environment> {
    environment: TEnvironment,
    pools: Arc<Mutex<HashMap<String, Arc<InitializedPluginPool<TEnvironment>>>>>,
    extension_to_plugin_name_map: Arc<Mutex<HashMap<String, String>>>,
}

impl<TEnvironment : Environment> PluginPools<TEnvironment> {
    pub fn new(environment: TEnvironment) -> Self {
        PluginPools {
            environment,
            pools: Arc::new(Mutex::new(HashMap::new())),
            extension_to_plugin_name_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn add_plugin(&self, plugin: Box<dyn Plugin>) {
        let plugin_name = String::from(plugin.name());
        let plugin_extensions = plugin.file_extensions().clone();
        let mut pools = self.pools.lock().unwrap();
        let mut extension_to_plugin_name_map = self.extension_to_plugin_name_map.lock().unwrap();
        pools.insert(plugin_name.clone(), Arc::new(InitializedPluginPool::new(plugin, self.environment.clone())));
        for extension in plugin_extensions.into_iter() {
            // first added plugin takes precedence
            if !extension_to_plugin_name_map.contains_key(&extension) {
                extension_to_plugin_name_map.insert(extension, plugin_name.clone());
            }
        }
    }

    pub fn get_pool(&self, plugin_name: &str) -> Option<Arc<InitializedPluginPool<TEnvironment>>> {
        self.pools.lock().unwrap().get(plugin_name).map(|p| p.clone())
    }

    pub fn get_plugin_name_from_extension(&self, ext: &str) -> Option<String> {
        self.extension_to_plugin_name_map.lock().unwrap().get(ext).map(|name| name.to_owned())
    }
}

pub struct InitializedPluginPool<TEnvironment : Environment> {
    environment: TEnvironment,
    plugin: Box<dyn Plugin>,
    items: AsyncMutex<Vec<Box<dyn InitializedPlugin>>>,
    semaphore: Semaphore,
}

impl<TEnvironment : Environment> InitializedPluginPool<TEnvironment> {
    pub fn new(plugin: Box<dyn Plugin>, environment: TEnvironment) -> InitializedPluginPool<TEnvironment> {
        // There is a performance cost associated with initializing a
        // plugin, so for now it's being limited to 2 instances per plugin
        let capacity = 2;
        InitializedPluginPool {
            environment,
            plugin: plugin,
            items: AsyncMutex::new(Vec::with_capacity(capacity)),
            semaphore: Semaphore::new(capacity),
        }
    }

    pub async fn initialize_first(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let initialized_plugin = self.force_create_instance()?;
        self.wait_and_reduce_semaphore().await?;
        Ok(initialized_plugin)
    }

    pub async fn try_take(&self) -> Result<Option<Box<dyn InitializedPlugin>>, ErrBox> {
        self.wait_and_reduce_semaphore().await?;

        let mut items = self.items.lock().await;
        // try to get an item from the pool
        return Ok(items.pop());
    }

    pub async fn create_pool_item(&self) -> Result<(), ErrBox> {
        self.release(self.force_create_instance()?).await;
        Ok(())
    }

    pub async fn release(&self, plugin: Box<dyn InitializedPlugin>) {
        let mut items = self.items.lock().await;
        items.push(plugin);
        self.semaphore.add_permits(1);
    }

    async fn wait_and_reduce_semaphore(&self) -> Result<(), ErrBox> {
        let permit = self.semaphore.acquire().await;
        permit.forget(); // reduce the number of permits (consumers must call .release)
        Ok(())
    }

    pub fn force_create_instance(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let start_instant = std::time::Instant::now();
        log_verbose!(self.environment, "Creating instance of {}", self.plugin.name());
        let result = self.plugin.initialize();
        log_verbose!(self.environment, "Created instance of {} in {}ms", self.plugin.name(), start_instant.elapsed().as_millis());
        result
    }
}
