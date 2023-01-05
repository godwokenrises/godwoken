import { Response } from "express";

export interface ResponseHeader {
  instantFinality: boolean;
  // add more below if needed
}

export function setResponseHeader(res: Response, header: ResponseHeader) {
  res.setHeader("X-Instant-Finality", header.instantFinality.toString());
}
