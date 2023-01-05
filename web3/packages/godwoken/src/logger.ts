import util from "util";
import winston, { format } from "winston";

// Don't import from `envConfig`
const loggerEnv: { [key: string]: string | undefined } = {
  logLevel: process.env.LOG_LEVEL,
  logFormat: process.env.LOG_FORMAT,
};

const normalFormat = format.printf(({ level, message, timestamp }) => {
  return `${timestamp} [${level}]: ${message}`;
});

// For development, set default log level to debug
// For production, set default log level to info
let logLevel = loggerEnv.logLevel;
if (logLevel == null && process.env.NODE_ENV === "production") {
  logLevel = "info";
} else if (logLevel == null) {
  logLevel = "debug";
}

let logFormat: winston.Logform.Format = format.combine(
  format.colorize(),
  format.timestamp(),
  normalFormat
);
if (loggerEnv.logFormat === "json") {
  logFormat = format.combine(
    format.uncolorize(),
    format.timestamp(),
    format.json()
  );
}

// Export for api-server
export const winstonLogger = winston.createLogger({
  level: logLevel,
  format: logFormat,
  transports: [new winston.transports.Console()],
});

const formatArgs = (args: any[]): string =>
  args.map((arg) => util.format(arg)).join(" ");

export const logger = {
  debug: (...args: any[]) => winstonLogger.debug(formatArgs(args)),
  info: (...args: any[]) => winstonLogger.info(formatArgs(args)),
  warn: (...args: any[]) => winstonLogger.warn(formatArgs(args)),
  error: (...args: any[]) => winstonLogger.error(formatArgs(args)),
};
