use crate::pipeline::PipelineComponents;
use crate::{BootError, ControllerDefinition, Module, ModuleRef, Result, RouteDefinition};
use std::collections::BTreeSet;
use std::sync::Arc;

pub(super) fn register_module(
    module: Arc<dyn Module>,
    module_ref: &ModuleRef,
    global_pipeline: &PipelineComponents,
    seen: &mut BTreeSet<String>,
    modules: &mut Vec<String>,
    module_instances: &mut Vec<Arc<dyn Module>>,
    routes: &mut Vec<RouteDefinition>,
) -> Result<()> {
    let name = module.name();
    if name.trim().is_empty() {
        return Err(BootError::EmptyModuleName);
    }
    if !seen.insert(name.to_string()) {
        return Ok(());
    }

    for imported in module.imports() {
        register_module(
            imported,
            module_ref,
            global_pipeline,
            seen,
            modules,
            module_instances,
            routes,
        )?;
    }

    for provider in module.providers()? {
        module_ref.register(provider)?;
    }

    module.on_module_init(module_ref)?;

    for controller in module.controllers(module_ref)? {
        register_controller(name, controller, global_pipeline, routes);
    }

    routes.extend(module.routes()?.into_iter().map(|route| {
        route
            .with_pipeline_prefix(global_pipeline)
            .with_module_name(name)
    }));
    modules.push(name.to_string());
    module_instances.push(module);
    Ok(())
}

fn register_controller(
    module_name: &str,
    controller: ControllerDefinition,
    global_pipeline: &PipelineComponents,
    routes: &mut Vec<RouteDefinition>,
) {
    routes.extend(controller.into_routes().into_iter().map(|route| {
        route
            .with_pipeline_prefix(global_pipeline)
            .with_module_name(module_name)
    }));
}
