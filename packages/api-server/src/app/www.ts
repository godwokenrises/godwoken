/**
 * Module dependencies.
 */

import { startServer } from "./app";
import { envConfig } from "../base/env-config";

/**
 * Get port from environment and store in Express.
 */
const port: number = +(envConfig.port || "8024");
startServer(port);
