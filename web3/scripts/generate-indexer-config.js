const dotenv = require("dotenv");
const path = require("path");
const fs = require("fs");
const http = require("http");
const crypto = require("crypto");

const envPath = path.join(__dirname, "../packages/api-server/.env");

dotenv.config({ path: envPath });

function sendJsonRpc(rpcUrl, method, params) {
  const url = new URL(rpcUrl);

  return new Promise((resolve, reject) => {
    const data = JSON.stringify({
      id: crypto.randomUUID(),
      jsonrpc: "2.0",
      method: method,
      params: params,
    });

    const options = {
      hostname: url.hostname,
      port: url.port,
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Content-Length": data.length,
      },
    };

    const req = http.request(options, (res) => {
      res.on("data", (d) => {
        const res = JSON.parse(d.toString());
        if (res.error) {
          reject(res);
        } else {
          resolve(res.result);
        }
      });
    });

    req.on("error", (error) => {
      console.error(error);
    });

    req.write(data);
    req.end();
  });
}

const run = async () => {
  const nodeInfo = await sendJsonRpc(
    process.env.GODWOKEN_JSON_RPC,
    "gw_get_node_info",
    []
  );

  let config = {
    l2_sudt_type_script_hash: nodeInfo.gw_scripts.find(
      (s) => s.script_type === "l2_sudt"
    ).type_hash,
    polyjuice_type_script_hash: nodeInfo.backends.find(
      (s) => s.backend_type === "polyjuice"
    ).validator_script_type_hash,
    rollup_type_hash: nodeInfo.rollup_cell.type_hash,
    eth_account_lock_hash: nodeInfo.eoa_scripts.find(
      (s) => s.eoa_type === "eth"
    ).type_hash,
    chain_id: +nodeInfo.rollup_config.chain_id,

    godwoken_rpc_url: process.env.GODWOKEN_JSON_RPC,
    pg_url: process.env.DATABASE_URL,
    sentry_dsn: process.env.SENTRY_DNS,
    sentry_environment: process.env.SENTRY_ENVIRONMENT,
  };

  let tomlStr = "";

  for (const [key, value] of Object.entries(config)) {
    console.log(`[${key}]: ${value}`);
    if (value != null && key === "chain_id") {
      tomlStr += `${key}=${Number(value)}\n`;
      continue;
    }
    if (value != null) {
      tomlStr += `${key}="${value}"\n`;
    }
  }

  const outputPath = path.join(__dirname, "../indexer-config.toml");
  fs.writeFileSync(outputPath, tomlStr);
};

run();
