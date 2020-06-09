use crate::environment::Environment;
use crate::types::ErrBox;
use super::{Plugin, InitializedPlugin};
use tokio::sync::{Mutex, Semaphore};

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

    pub async fn initialize_first(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let initialized_plugin = self.create_instance()?;
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
        self.release(self.create_instance()?).await;
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

    fn create_instance(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        log_verbose!(self.environment, "Creating instance of {}", self.plugin.name());
        let result = self.plugin.initialize();
        log_verbose!(self.environment, "Created instance of {}", self.plugin.name());
        result
    }
}

// pub struct ExtensionToNameMap {
//     extensions_to_name: HashMap<String, String>,
// }

// impl ExtensionToNameMap {
//     pub fn new(plugins: &Vec<Box<dyn Plugin>>) -> ExtensionToNameMap {
//         let mut extensions_to_name = HashMap::new();
//         for plugin in plugins {
//             let plugin_name = plugin.name();
//             for file_extension in plugin.file_extensions() {
//                 // first takes presedence
//                 if !extensions_to_name.contains_key(file_extension) {
//                     extensions_to_name.insert(String::from(file_extension), String::from(plugin_name));
//                 }
//             }
//         }
//         ExtensionToNameMap {
//             extensions_to_name,
//         }
//     }
// }
