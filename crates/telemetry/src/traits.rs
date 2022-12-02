use tracing::Span;

pub use opentelemetry::{trace::TraceContextExt, Context};
pub use tracing_opentelemetry::OpenTelemetrySpanExt;

pub trait TelemetryContext {
    fn telemetry_context(&self) -> Option<&Context>;

    fn or_current(&self) -> Context {
        { self.telemetry_context().cloned() }.unwrap_or_else(crate::current_context)
    }
}

pub trait TelemetryContextNewSpan<CreateSpan, NewSpan> {
    fn new_span(&self, create_span: CreateSpan) -> NewSpan;
}

impl<C: TelemetryContext, F: Fn(&Context) -> Span> TelemetryContextNewSpan<F, Option<Span>> for C {
    /// Create span if there's context
    fn new_span(&self, create_span: F) -> Option<Span> {
        let ctx = self.telemetry_context()?;
        let span = create_span(ctx);
        span.set_parent(ctx.clone());
        Some(span)
    }
}

impl TelemetryContextNewSpan<Span, Span> for Context {
    fn new_span(&self, span: Span) -> Span {
        span.set_parent(self.clone());
        span
    }
}

impl TelemetryContext for () {
    fn telemetry_context(&self) -> Option<&Context> {
        None
    }
}

impl TelemetryContext for Context {
    fn telemetry_context(&self) -> Option<&Context> {
        Some(self)
    }
}

impl TelemetryContext for Option<Context> {
    fn telemetry_context(&self) -> Option<&Context> {
        self.as_ref()
    }
}

pub trait TelemetryContextRemote {
    fn with_remote_context(&self, remote: &Context) -> Context;
}

impl TelemetryContextRemote for Context {
    fn with_remote_context(&self, remote_ctx: &Context) -> Context {
        let span_ctx = remote_ctx.span().span_context().clone();
        self.with_remote_span_context(span_ctx)
    }
}

pub trait TelemetrySpanExt<'a> {
    type Entered;

    fn enter(&'a self) -> Self::Entered;
}

impl<'a> TelemetrySpanExt<'a> for Option<Span> {
    type Entered = Option<tracing::span::Entered<'a>>;

    fn enter(&'a self) -> Self::Entered {
        self.as_ref().map(|s| s.enter())
    }
}
