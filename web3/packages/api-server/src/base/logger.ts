import expressWinston from "express-winston";
import { logger, winstonLogger } from "@godwoken-web3/godwoken";

export { logger };

export const expressLogger = expressWinston.logger({
  transports: winstonLogger.transports,
  format: winstonLogger.format,
  meta: false, // optional: control whether you want to log the meta data about the request (default to true)
  msg: "{{req.method}} {{req.url}} {{res.statusCode}} {{res.responseTime}}ms @{{ Array.isArray(req.body) ? req.body.map((o) => o?.method) : req.body?.method }}", // optional: customize the default logging message. E.g. "{{res.statusCode}} {{req.method}} {{res.responseTime}}ms {{req.url}}"
  expressFormat: false, // Use the default Express/morgan request formatting. Enabling this will override any msg if true. Will only output colors with colorize set to true
  colorize: true, // Color the text and status code, using the Express/morgan color palette (text: gray, status: default green, 3XX cyan, 4XX yellow, 5XX red).
  // dynamicMeta: (req, res) => {
  //   const rpcRequest: any = {}
  //   const meta: any = {};
  //   if (req) {
  //     meta.rpc = rpcRequest
  //     rpcRequest.methods = Array.isArray(req.body) ? req.body.map((o) => o?.method) : [req.body?.method]
  //   }
  //   return meta
  // }
});
