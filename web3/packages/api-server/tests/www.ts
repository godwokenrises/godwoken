import jayson from "jayson/promise";

export const client = jayson.Client.http({
  port: process.env.PORT || "8024",
});

export interface JSONResponse {
  jsonrpc: "2.0";
  id: string;
  result?: any;
  error?: {
    code: number;
    message: string;
  };
}
