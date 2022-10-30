use tracing::Span;

pub use opentelemetry::{trace::TraceContextExt, Context};
pub use tracing_opentelemetry::OpenTelemetrySpanExt;

pub trait GwOtelContext {
    fn otel_context(&self) -> Option<&Context>;

    fn or_current(&self) -> Context {
        { self.otel_context().cloned() }.unwrap_or_else(crate::current_context)
    }
}

pub trait GwOtelContextNewSpan<CreateSpan, NewSpan> {
    fn new_span(&self, create_span: CreateSpan) -> NewSpan;
}

impl<C: GwOtelContext, F: Fn(&Context) -> Span> GwOtelContextNewSpan<F, Option<Span>> for C {
    /// Create span if there's context
    fn new_span(&self, create_span: F) -> Option<Span> {
        let ctx = self.otel_context()?;
        let span = create_span(ctx);
        span.set_parent(ctx.clone());
        Some(span)
    }
}

impl GwOtelContextNewSpan<Span, Span> for Context {
    fn new_span(&self, span: Span) -> Span {
        span.set_parent(self.clone());
        span
    }
}

impl GwOtelContext for () {
    fn otel_context(&self) -> Option<&Context> {
        None
    }
}

impl GwOtelContext for Context {
    fn otel_context(&self) -> Option<&Context> {
        Some(self)
    }
}

impl GwOtelContext for Option<Context> {
    fn otel_context(&self) -> Option<&Context> {
        self.as_ref()
    }
}

pub trait GwOtelContextRemote {
    fn with_remote_context(&self, remote: &Context) -> Context;
}

impl GwOtelContextRemote for Context {
    fn with_remote_context(&self, remote_ctx: &Context) -> Context {
        let span_ctx = remote_ctx.span().span_context().clone();
        self.with_remote_span_context(span_ctx)
    }
}

pub trait GwOtelSpanExt<'a> {
    type Entered;

    fn enter(&'a self) -> Self::Entered;
}

impl<'a> GwOtelSpanExt<'a> for Option<Span> {
    type Entered = Option<tracing::span::Entered<'a>>;

    fn enter(&'a self) -> Self::Entered {
        self.as_ref().map(|s| s.enter())
    }
}
