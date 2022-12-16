import process from "process";

import * as opentelemetry from "@opentelemetry/sdk-node";

import { ExpressInstrumentation } from "@opentelemetry/instrumentation-express";
import { HttpInstrumentation } from "@opentelemetry/instrumentation-http";
import { KnexInstrumentation } from "@opentelemetry/instrumentation-knex";
import { RedisInstrumentation } from "@opentelemetry/instrumentation-redis-4";
import { WinstonInstrumentation } from "@opentelemetry/instrumentation-winston";

import { JaegerExporter } from "@opentelemetry/exporter-jaeger";
import { JaegerPropagator } from "@opentelemetry/propagator-jaeger";

// TODO: We can remove this after upgrade sdk-node to 0.34
// Reference: https://github.com/open-telemetry/opentelemetry-js/pull/3388
const jaegerExporter = new JaegerExporter();
const jaegerPropagator = new JaegerPropagator();

// Configuration (sdk 0.34 or later):
// Disable:
// OTEL_TRACES_EXPORTER: none
//
// Example(jaeger):
// LOG_FORMAT: json (add trace id to log)
// OTEL_TRACES_EXPORTER: jaeger
// OTEL_PROPAGATORS: jaeger
// OTEL_EXPORTER_OTLP_PROTOCOL: grpc (default is http/protobuf)
// OTEL_EXPORTER_JAEGER_ENDPOINT: http://jaeger:14250
// OTEL_RESOURCE_ATTRIBUTES: service.name=web3-readonly1
//
// Reference: https://github.com/open-telemetry/opentelemetry-js/blob/main/experimental/packages/opentelemetry-sdk-node/README.md
const sdk = new opentelemetry.NodeSDK({
  traceExporter: jaegerExporter,
  textMapPropagator: jaegerPropagator,
  instrumentations: [
    new HttpInstrumentation(),
    new RedisInstrumentation(),
    new ExpressInstrumentation(),
    new KnexInstrumentation(),
    // Add trace_id, span_id and trace_flags to log entry
    new WinstonInstrumentation(),
  ],
});

export function startOpentelemetry() {
  sdk
    .start()
    .then(() => console.log("Opentelemetry tracing initialized"))
    .catch((error) =>
      console.log("Error initializing opentelemetry tracing", error)
    );

  process.on("SIGTERM", () => {
    sdk
      .shutdown()
      .then(() => console.log("Opentelemetry Tracing terminated"))
      .catch((error) =>
        console.log("Error terminating opentelemetry tracing", error)
      )
      .finally(() => process.exit(0));
  });
}
