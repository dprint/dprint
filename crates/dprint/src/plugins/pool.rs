use std::sync::Arc;
use std::collections::HashMap;
use parking_lot::Mutex;
use tokio::sync::Semaphore;

use dprint_core::types::ErrBox;

use crate::environment::Environment;
use super::{Plugin, InitializedPlugin};

/// This is necessary because of a circular reference where
/// PluginPools hold plugins and the plugins hold a PluginPools.
pub struct PluginsDropper<TEnvironment: Environment> {
    pools: Arc<PluginPools<TEnvironment>>,
}

impl<TEnvironment: Environment> Drop for PluginsDropper<TEnvironment> {
    fn drop(&mut self) {
        self.pools.drop_plugins();
    }
}

impl<TEnvironment: Environment> PluginsDropper<TEnvironment> {
    pub fn new(pools: Arc<PluginPools<TEnvironment>>) -> Self {
        PluginsDropper { pools }
    }
}

pub struct PluginPools<TEnvironment : Environment> {
    environment: TEnvironment,
    pools: Mutex<HashMap<String, Arc<InitializedPluginPool<TEnvironment>>>>,
    extension_to_plugin_name_map: Mutex<HashMap<String, String>>,
    /// Plugins may format using other plugins. Since when plugins are formatting other plugins
    /// they cannot use an async operation, they must be provided with an instance synchronously
    /// and cannot get a plugin from the plugin pool below.
    plugins_for_plugins: Mutex<HashMap<String, HashMap<String, Vec<Box<dyn InitializedPlugin>>>>>,
}

impl<TEnvironment : Environment> PluginPools<TEnvironment> {
    pub fn new(environment: TEnvironment) -> Self {
        PluginPools {
            environment,
            pools: Mutex::new(HashMap::new()),
            extension_to_plugin_name_map: Mutex::new(HashMap::new()),
            plugins_for_plugins: Mutex::new(HashMap::new()),
        }
    }

    pub fn drop_plugins(&self) {
        {
            let mut pools = self.pools.lock();
            for pool in pools.values() {
                pool.drop_plugins();
            }
            pools.clear();
        }
        {
            let mut plugins_for_plugins = self.plugins_for_plugins.lock();
            plugins_for_plugins.clear();
        }
    }

    pub fn set_plugins(&self, plugins: Vec<Box<dyn Plugin>>) {
        let mut pools = self.pools.lock();
        let mut extension_to_plugin_name_map = self.extension_to_plugin_name_map.lock();
        for plugin in plugins {
            let plugin_name = String::from(plugin.name());
            let plugin_extensions = plugin.file_extensions().clone();
            pools.insert(plugin_name.clone(), Arc::new(InitializedPluginPool::new(plugin, self.environment.clone())));
            for extension in plugin_extensions.into_iter() {
                // first added plugin takes precedence
                if !extension_to_plugin_name_map.contains_key(&extension) {
                    extension_to_plugin_name_map.insert(extension, plugin_name.clone());
                }
            }
        }
    }

    pub fn get_pool(&self, plugin_name: &str) -> Option<Arc<InitializedPluginPool<TEnvironment>>> {
        self.pools.lock().get(plugin_name).map(|p| p.clone())
    }

    pub fn take_instance_for_plugin(&self, parent_plugin_name: &str, sub_plugin_name: &str) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let plugin = {
            let mut plugins_for_plugins = self.plugins_for_plugins.lock(); // keep this lock short
            let plugins_for_plugin = if let Some(plugins_for_plugin) = plugins_for_plugins.get_mut(parent_plugin_name) {
                plugins_for_plugin
            } else {
                plugins_for_plugins.insert(parent_plugin_name.to_string(), HashMap::new());
                plugins_for_plugins.get_mut(parent_plugin_name).unwrap()
            };
            let plugins = if let Some(plugins) = plugins_for_plugin.get_mut(sub_plugin_name) {
                plugins
            } else {
                plugins_for_plugin.insert(sub_plugin_name.to_string(), Vec::new());
                plugins_for_plugin.get_mut(sub_plugin_name).unwrap()
            };
            plugins.pop()
        };

        if let Some(plugin) = plugin {
            Ok(plugin)
        } else {
            let pool = self.get_pool(sub_plugin_name).expect("Expected the plugin to exist in the pool.");
            pool.force_create_instance()
        }
    }

    pub fn release_instance_for_plugin(&self, parent_plugin_name: &str, sub_plugin_name: &str, plugin: Box<dyn InitializedPlugin>) {
        let mut plugins_for_plugins = self.plugins_for_plugins.lock();
        let plugins_for_plugin = plugins_for_plugins.get_mut(parent_plugin_name).unwrap();
        let plugins = plugins_for_plugin.get_mut(sub_plugin_name).unwrap();
        plugins.push(plugin);
    }

    pub fn get_plugin_name_from_extension(&self, ext: &str) -> Option<String> {
        self.extension_to_plugin_name_map.lock().get(ext).map(|name| name.to_owned())
    }

    pub fn release(&self, parent_plugin_name: &str) {
        let plugins_for_plugin = self.plugins_for_plugins.lock().remove(parent_plugin_name);
        if let Some(plugins_for_plugin) = plugins_for_plugin {
            for (sub_plugin_name, initialized_plugins) in plugins_for_plugin.into_iter() {
                if let Some(pool) = self.get_pool(&sub_plugin_name) {
                    pool.release_all(initialized_plugins);
                }
            }
        }
    }

    /// Gets a hash to be used for the "incremental" feature to tell if any plugins have changed.
    pub fn get_plugins_hash(&self) -> u64 {
        use std::num::Wrapping;
        // yeah, I know adding hashes isn't right, but the chance of this not working
        // in order to tell when a plugin has changed is super low.
        let pools = self.pools.lock();
        let mut hash_sum = Wrapping(0);
        for (_, pool) in pools.iter() {
            hash_sum += Wrapping(pool.plugin.get_hash());
        }
        hash_sum.0
    }
}

pub struct InitializedPluginPool<TEnvironment : Environment> {
    environment: TEnvironment,
    plugin: Box<dyn Plugin>,
    items: Mutex<Vec<Box<dyn InitializedPlugin>>>,
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
            items: Mutex::new(Vec::with_capacity(capacity)),
            semaphore: Semaphore::new(capacity),
        }
    }

    pub fn drop_plugins(&self) {
        let mut items = self.items.lock();
        items.clear();
    }

    pub fn take_or_create_no_sempahore(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let mut items = self.items.lock();
        if let Some(item) = items.pop() {
            Ok(item)
        } else {
            self.force_create_instance()
        }
    }

    pub fn release_no_semaphore(&self, plugin: Box<dyn InitializedPlugin>) {
        let mut items = self.items.lock();
        items.push(plugin);
    }

    pub async fn initialize_first(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let initialized_plugin = self.force_create_instance()?;
        self.wait_and_reduce_semaphore().await?;
        Ok(initialized_plugin)
    }

    pub async fn try_take(&self) -> Result<Option<Box<dyn InitializedPlugin>>, ErrBox> {
        self.wait_and_reduce_semaphore().await?;

        let mut items = self.items.lock();
        // try to get an item from the pool
        return Ok(items.pop());
    }

    pub fn create_pool_item(&self) -> Result<(), ErrBox> {
        self.release(self.force_create_instance()?);
        Ok(())
    }

    pub fn release(&self, plugin: Box<dyn InitializedPlugin>) {
        let mut items = self.items.lock();
        items.push(plugin);
        self.semaphore.add_permits(1);
    }

    pub fn release_all(&self, plugins: Vec<Box<dyn InitializedPlugin>>) {
        let mut items = self.items.lock();
        let plugins_len = plugins.len();
        items.extend(plugins);
        self.semaphore.add_permits(plugins_len);
    }

    async fn wait_and_reduce_semaphore(&self) -> Result<(), ErrBox> {
        let permit = self.semaphore.acquire().await;
        permit.forget(); // reduce the number of permits (consumers must call .release)
        Ok(())
    }

    pub fn force_create_instance(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let start_instant = std::time::Instant::now();
        log_verbose!(self.environment, "Creating instance of {}", self.plugin.name());
        let plugin = self.plugin.initialize()?;
        log_verbose!(self.environment, "Created instance of {} in {}ms", self.plugin.name(), start_instant.elapsed().as_millis());
        Ok(plugin)
    }
}
