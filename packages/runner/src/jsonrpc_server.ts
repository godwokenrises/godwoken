import { ChainService } from "@ckb-godwoken/godwoken";
import jayson from "jayson";
import cors from "cors";
import connect from "connect";
import { json } from "body-parser";

export class JsonrpcServer {
  chainService: ChainService;
  server: jayson.Server;
  listen: string;

  constructor(chainService: ChainService, listen: string) {
    this.chainService = chainService;
    this.server = new jayson.Server({});
    this.listen = listen;
  }

  async start() {
    const app = connect();
    app.use(cors({ methods: ["GET", "PUT", "POST"] }));
    app.use(json());
    app.use(this.server.middleware());

    app.listen(this.listen);
  }
}
