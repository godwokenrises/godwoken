use neon::prelude::*;

mod chain {
    pub mod chain;
}

use chain::chain::*;

register_module!(mut cx, {
    cx.export_class::<JsNativeChain>("NativeChain")?;
    Ok(())
});
