pub mod metric;
pub mod trace;
pub mod traits;

pub mod opentelemetry {
    pub use opentelemetry::*;
    pub use tracing_opentelemetry::*;
}
pub use tracing;

pub use crate::opentelemetry::global;
pub use crate::opentelemetry::Context;

use crate::opentelemetry::propagation::Extractor;
pub fn extract_context(extractor: &dyn Extractor) -> opentelemetry::Context {
    crate::opentelemetry::global::get_text_map_propagator(|p| p.extract(extractor))
}

pub fn current_span() -> tracing::Span {
    tracing::Span::current()
}

pub fn current_context() -> Context {
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    crate::current_span().context()
}

use ::opentelemetry::trace::SpanRef;
pub fn with_span_ref<F, O>(span: &tracing::Span, f: F) -> O
where
    F: Fn(&SpanRef<'_>) -> O,
{
    use ::opentelemetry::trace::TraceContextExt;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let ctx = span.context();
    let span = ctx.span();
    f(&span)
}
