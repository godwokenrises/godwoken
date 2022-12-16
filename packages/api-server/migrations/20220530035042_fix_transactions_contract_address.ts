import { Knex } from "knex";

export async function up(knex: Knex): Promise<void> {
  await knex("transactions")
    .update({ contract_address: null })
    .where({ contract_address: Buffer.from("", "hex") });
}

export async function down(knex: Knex): Promise<void> {
  await knex("transactions")
    .update({ contract_address: Buffer.from("", "hex") })
    .where({ contract_address: null });
}
