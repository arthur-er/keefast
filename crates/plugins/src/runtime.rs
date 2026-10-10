//! A Plugin's WASM component, run in wasmtime: one instance per Plugin, with a memory cap and a
//! CPU limit (fuel) per hook call. The linker gives it nothing to import, so a component that
//! wants WASI or anything else (a filesystem, the network, a clock) doesn't load.

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

use crate::PluginError;

mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "plugin",
    });
}

pub use bindings::keefast::plugin::types::{Derived, Field, Problem, Severity};

/// The most linear memory an instance may have.
pub const MEMORY_CAP: usize = 16 << 20;
/// How much a hook call may compute: some tens of millions of instructions, far more than
/// formatting or labelling a few fields takes, and a few milliseconds' work.
pub const FUEL_PER_CALL: u64 = 20_000_000;

/// The engine Plugins are compiled and run with.
pub fn engine() -> Engine {
    let mut config = Config::new();
    config.consume_fuel(true);
    Engine::new(&config).expect("the Plugin engine's configuration is valid")
}

struct State {
    limits: StoreLimits,
}

/// A running instance, and the store it lives in.
struct Instance {
    store: Store<State>,
    hooks: bindings::Plugin,
}

/// A Plugin's compiled component, instantiated when a hook is first called, and again after a
/// call fails (a trap leaves an instance unusable).
pub struct Runtime {
    engine: Engine,
    pre: bindings::PluginPre<State>,
    instance: Option<Instance>,
}

impl Runtime {
    /// Compiles a component, checking it is a Plugin of this API version that imports nothing.
    pub fn new(engine: &Engine, bytes: &[u8]) -> Result<Self, PluginError> {
        let component = Component::new(engine, bytes)
            .map_err(|error| PluginError::Component(format!("{error:#}")))?;
        let linker = Linker::<State>::new(engine);
        let pre = linker
            .instantiate_pre(&component)
            .and_then(bindings::PluginPre::new)
            .map_err(|error| PluginError::Component(format!("{error:#}")))?;
        Ok(Self {
            engine: engine.clone(),
            pre,
            instance: None,
        })
    }

    /// Calls a hook with a fresh allowance of fuel. A failed call drops the instance.
    fn call<T>(
        &mut self,
        hook: impl FnOnce(&bindings::Plugin, &mut Store<State>) -> wasmtime::Result<T>,
    ) -> Result<T, PluginError> {
        if self.instance.is_none() {
            let limits = StoreLimitsBuilder::new()
                .memory_size(MEMORY_CAP)
                .instances(8)
                .memories(4)
                .tables(8)
                .trap_on_grow_failure(true)
                .build();
            let mut store = Store::new(&self.engine, State { limits });
            store.limiter(|state| &mut state.limits);
            store.set_fuel(FUEL_PER_CALL).map_err(hook_error)?;
            let hooks = self.pre.instantiate(&mut store).map_err(hook_error)?;
            self.instance = Some(Instance { store, hooks });
        }
        let instance = self.instance.as_mut().expect("instantiated above");
        instance.store.set_fuel(FUEL_PER_CALL).map_err(hook_error)?;
        let result = hook(&instance.hooks, &mut instance.store);
        if result.is_err() {
            self.instance = None;
        }
        result.map_err(hook_error)
    }

    pub fn format(&mut self, facet: &str, key: &str, value: &str) -> Result<String, PluginError> {
        self.call(|hooks, store| hooks.call_format(store, facet, key, value))
    }

    pub fn validate(
        &mut self,
        facet: &str,
        key: &str,
        value: &str,
    ) -> Result<Option<Problem>, PluginError> {
        self.call(|hooks, store| hooks.call_validate(store, facet, key, value))
    }

    pub fn derive(&mut self, facet: &str, fields: &[Field]) -> Result<Derived, PluginError> {
        self.call(|hooks, store| hooks.call_derive(store, facet, fields))
    }
}

fn hook_error(error: wasmtime::Error) -> PluginError {
    PluginError::Hook(format!("{error:#}"))
}
