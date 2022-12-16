import { Knex } from "knex";

// u32: bigint(i64)
// u64: decimal(20, 0)
// u128: decimal(40, 0)
// u256: decimal(80, 0)
export async function up(knex: Knex): Promise<void> {
  await knex.schema
    .createTable("blocks", function (table: Knex.TableBuilder) {
      table.decimal("number", null, 0).primary().notNullable();
      table.binary("hash").notNullable().unique();
      table.binary("parent_hash").notNullable();
      table.decimal("gas_limit", null, 0).notNullable();
      table.decimal("gas_used", null, 0).notNullable();
      table.binary("miner").notNullable();
      table.integer("size").notNullable();
      table.timestamp("timestamp").notNullable();
    })
    .createTable("transactions", function (table: Knex.TableBuilder) {
      table.bigIncrements("id");
      table.binary("hash").notNullable().unique();
      table.binary("eth_tx_hash").notNullable().unique();
      table
        .decimal("block_number", null, 0)
        .notNullable()
        .references("blocks.number");
      table.binary("block_hash").notNullable();
      table.integer("transaction_index").notNullable();
      table.binary("from_address").notNullable().index();
      table.binary("to_address").index();
      // value: uint256
      table.decimal("value", 80, 0).notNullable();
      table.bigInteger("nonce").notNullable();
      table.decimal("gas_limit", null, 0);
      table.decimal("gas_price", null, 0);
      table.binary("input");
      table.smallint("v").notNullable();
      table.binary("r").notNullable();
      table.binary("s").notNullable();
      table.decimal("cumulative_gas_used", null, 0);
      table.decimal("gas_used", null, 0);
      table.binary("contract_address").index();
      table.smallint("exit_code").notNullable();
      table.unique(["block_hash", "transaction_index"], {
        indexName: "block_hash_transaction_index_idx",
      });
      table.unique(["block_number", "transaction_index"], {
        indexName: "block_number_transaction_index_idx",
      });
    })
    .createTable("logs", function (table: Knex.TableBuilder) {
      table.bigIncrements("id");
      table
        .bigInteger("transaction_id")
        .notNullable()
        .index()
        .references("transactions.id");
      table.binary("transaction_hash").notNullable().index();
      table.integer("transaction_index").notNullable();
      table
        .decimal("block_number", null, 0)
        .notNullable()
        .index()
        .references("blocks.number");
      table.binary("block_hash").notNullable().index();
      table.binary("address").notNullable().index();
      table.binary("data");
      table.integer("log_index").notNullable();
      table.specificType("topics", "bytea ARRAY").notNullable();
    });
}

export async function down(knex: Knex): Promise<void> {
  await knex.schema
    .dropTable("logs")
    .dropTable("transactions")
    .dropTable("blocks");
}
