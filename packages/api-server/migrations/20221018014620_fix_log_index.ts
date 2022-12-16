import { Knex } from "knex";

// Fix for wrong logs.log_index
export async function up(knex: Knex): Promise<void> {
  await knex.raw(
    "with cte as(select block_number, id, log_index, row_number() over (partition by block_number order by id) rn from logs) update logs set log_index=cte.rn - 1 from cte where logs.block_number=cte.block_number and logs.id=cte.id;"
  );
}

export async function down(knex: Knex): Promise<void> {}
