use super::{get_builtins, BuiltinFunctionsMap};
use crate::errors::{BurregoError, Result};

use lazy_static::lazy_static;
use std::sync::RwLock;
use tracing::debug;

lazy_static! {
    pub(crate) static ref BUILTINS_HELPER: RwLock<BuiltinsHelper> = {
        RwLock::new(BuiltinsHelper {
            builtins: get_builtins(),
        })
    };
}
pub(crate) struct BuiltinsHelper {
    builtins: BuiltinFunctionsMap,
}

impl BuiltinsHelper {
    pub(crate) fn invoke(
        &self,
        builtin_name: &str,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value> {
        let builtin_fn = self
            .builtins
            .get(builtin_name)
            .ok_or_else(|| BurregoError::BuiltinNotImplementedError(builtin_name.to_string()))?;

        debug!(
            builtin = builtin_name,
            args = serde_json::to_string(&args)
                .expect("cannot convert builtins args to JSON")
                .as_str(),
            "invoking builtin"
        );
        builtin_fn(args)
    }
}
