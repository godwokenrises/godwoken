import { Knex } from "knex";

export async function up(knex: Knex): Promise<void> {
  await knex.schema
    .alterTable("transactions", (table) => {
      table.dropForeign("block_number");
    })
    .alterTable("logs", (table) => {
      table.dropForeign("block_number").dropForeign("transaction_id");
    });
}

export async function down(knex: Knex): Promise<void> {
  await knex.schema
    .alterTable("transactions", (table) => {
      table.foreign("block_number").references("blocks.number");
    })
    .alterTable("logs", (table) => {
      table.foreign("block_number").references("blocks.number");
    })
    .alterTable("logs", (table) => {
      table.foreign("transaction_id").references("transactions.id");
    });
}
