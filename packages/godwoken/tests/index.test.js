const test = require("ava");
const fs = require("fs");
const path = require("path");
const { ChainService } = require("../lib");
const configPath = path.join(__dirname, "config.json");

test("Init a chain by config", (t) => {
  let rawData = fs.readFileSync(configPath);
  let config = JSON.parse(rawData);
  let chainService = new ChainService(config);
  t.pass();
});
