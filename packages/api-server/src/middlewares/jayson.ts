import jayson from "jayson";
import { instantFinalityHackMethods, methods } from "../methods/index";
import { Request, Response, NextFunction } from "express";
import createServer from "connect";
import { isInstantFinalityHackMode } from "../util";
import { ResponseHeader, setResponseHeader } from "./header";

const server = new jayson.Server(methods);
const instantFinalityHackServer = new jayson.Server(instantFinalityHackMethods);

export const jaysonMiddleware = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  // we use jayson, and {params: null} will be treated as illegal while undefined will not.
  // because this line of code https://github.com/tedeh/jayson/blob/master/lib/utils.js#L331
  if (req.body && req.body.params == null) {
    req.body.params = [] as any[];
  }

  const enableInstantFinality = isInstantFinalityHackMode(req);

  // manage response header here
  const header: ResponseHeader = {
    instantFinality: enableInstantFinality,
  };
  setResponseHeader(res, header);

  // enable additional feature for special URL
  if (enableInstantFinality) {
    const middleware =
      instantFinalityHackServer.middleware() as createServer.NextHandleFunction;
    return middleware(req, res, next);
  }

  const middleware = server.middleware() as createServer.NextHandleFunction;
  return middleware(req, res, next);
};
