use chrono::{SecondsFormat, Utc};
use opentelemetry::trace::{SpanBuilder, SpanId, TraceContextExt, TraceId};
use serde::ser::{SerializeMap, Serializer as _};
use tracing::{Event, Subscriber};
use tracing_opentelemetry::OtelData;
use tracing_serde::fields::AsMap;
use tracing_serde::AsSerde;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::registry::{LookupSpan, SpanRef};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io;

pub struct TraceFormat;

impl<S, N> FormatEvent<S, N> for TraceFormat
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        let meta = event.metadata();

        let mut visit = || {
            let mut serializer = serde_json::Serializer::new(WriteUtf8Str::new(&mut writer));
            let mut serializer = serializer.serialize_map(None)?;

            serializer.serialize_entry(
                "timestamp",
                &Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            )?;
            serializer.serialize_entry("level", &meta.level().as_serde())?;
            serializer.serialize_entry("fields", &event.field_map())?;
            serializer.serialize_entry("target", meta.target())?;

            if let Some(span_ref) = ctx.lookup_current() {
                if let Some(TraceInfo { trace_id, span_id }) = lookup_trace_info(&span_ref) {
                    serializer.serialize_entry("trace_id", trace_id.as_str())?;
                    serializer.serialize_entry("span_id", span_id.as_str())?;
                }

                // Serialize current span name
                serializer.serialize_entry("span_name", span_ref.name())?;

                // Serialize spans' fields from current to root
                #[derive(serde::Deserialize)]
                #[serde(transparent)]
                struct SpanFields<'a> {
                    #[serde(borrow)]
                    fields: BTreeMap<Cow<'a, str>, Cow<'a, serde_json::value::RawValue>>,
                }

                let mut parent_span_ref = Some(span_ref);
                while let Some(span_ref) = parent_span_ref.take() {
                    let span_exts = span_ref.extensions();
                    let fmt_fields = match span_exts.get::<FormattedFields<N>>() {
                        Some(fields) if !fields.is_empty() => fields,
                        _ => continue,
                    };

                    let span_fields = match serde_json::from_str(fmt_fields) {
                        Ok(SpanFields { fields }) if !fields.is_empty() => fields,
                        _ => continue,
                    };

                    // ignore unprintable span fields error
                    let _ = serializer.serialize_entry(span_ref.name(), &span_fields);
                    parent_span_ref = span_ref.parent();
                }
            }

            serializer.end()
        };

        visit().map_err(|_| std::fmt::Error)?;
        writeln!(writer)
    }
}

struct HexId {
    buf: [u8; 32],
    len: usize,
}

impl HexId {
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[..self.len]).expect("valid buf from faster-hex")
    }
}

struct TraceInfo {
    trace_id: HexId,
    span_id: HexId,
}

fn lookup_trace_id<S>(span_ref: &SpanRef<S>) -> Option<TraceId>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    // Lookup from current span
    let span_ext = span_ref.extensions();
    let ot_data = span_ext.get::<OtelData>()?;
    let id = ot_data.parent_cx.span().span_context().trace_id();
    if TraceId::INVALID != id {
        return Some(id);
    }

    // From span builder in data
    if let Some(id) = ot_data.builder.trace_id {
        if TraceId::INVALID != id {
            return Some(id);
        }
    }

    // Iterate parent span builders until find valid trace id
    let find_id = |span_ref: &SpanRef<S>| -> Option<TraceId> {
        { span_ref.extensions() }
            .get::<SpanBuilder>()
            .map(|b| b.trace_id)?
    };
    if let Some(id) = find_id(span_ref) {
        return Some(id);
    }

    let mut parent_span_ref = span_ref.parent()?;
    loop {
        match find_id(&parent_span_ref) {
            Some(id) => return Some(id),
            None => parent_span_ref = parent_span_ref.parent()?,
        }
    }
}

fn lookup_trace_info<S>(span_ref: &SpanRef<S>) -> Option<TraceInfo>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    let span_ext = span_ref.extensions();
    let ot_data = span_ext.get::<OtelData>()?;

    let to_hex = |id: &[u8]| -> Option<HexId> {
        let mut buf = [0u8; 32];
        let len = id.len() * 2;
        assert!(len <= 32);

        faster_hex::hex_encode(id, &mut buf).ok()?;
        Some(HexId { buf, len })
    };

    let trace_id = to_hex(&lookup_trace_id(span_ref)?.to_bytes())?;
    let span_id = to_hex(
        &{ ot_data.builder.span_id }
            .unwrap_or(SpanId::INVALID)
            .to_bytes(),
    )?;

    Some(TraceInfo { trace_id, span_id })
}

struct WriteUtf8Str<'a> {
    fmt_write: &'a mut dyn std::fmt::Write,
}

impl<'a> WriteUtf8Str<'a> {
    pub fn new(fmt_write: &'a mut dyn std::fmt::Write) -> Self {
        Self { fmt_write }
    }
}

impl<'a> io::Write for WriteUtf8Str<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let s =
            std::str::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        self.fmt_write
            .write_str(s)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(s.as_bytes().len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
