import Knex, { Knex as KnexType } from "knex";
import { DBLog } from "../src/db/types";
import commander from "commander";
import dotenv from "dotenv";

export async function fixLogTransactionIndexRun(program: commander.Command) {
  try {
    let databaseUrl = (program as any).databaseUrl;
    if (databaseUrl == null) {
      dotenv.config({ path: "./.env" });
      databaseUrl = process.env.DATABASE_URL;
    }

    if (databaseUrl == null) {
      throw new Error("Please provide --database-url");
    }

    await fixLogTransactionIndex(databaseUrl);
    process.exit(0);
  } catch (e) {
    console.error(e);
    process.exit(1);
  }
}

export async function wrongLogTransactionIndexCountRun(
  program: commander.Command
): Promise<void> {
  try {
    let databaseUrl = (program as any).databaseUrl;
    if (databaseUrl == null) {
      dotenv.config({ path: "./.env" });
      databaseUrl = process.env.DATABASE_URL;
    }
    if (databaseUrl == null) {
      throw new Error("Please provide --database-url");
    }
    await wrongLogTransactionIndexCount(databaseUrl);
    process.exit(0);
  } catch (e) {
    console.error(e);
    process.exit(1);
  }
}

// fix for leading zeros
export async function fixLogTransactionIndex(
  databaseUrl: string
): Promise<void> {
  const knex = getKnex(databaseUrl);

  const query = knex<DBLog>("logs")
    .whereRaw(
      "transaction_index <> (select transaction_index from transactions where hash = logs.transaction_hash)"
    )
    .count();
  const sql = query.toSQL();
  console.log("Query SQL:", sql.sql);
  const wrongLogsCount = await query;
  console.log(`Found ${wrongLogsCount[0].count} wrong logs`);

  const _updateQuery = await knex.raw(
    "update logs set transaction_index = subquery.transaction_index from (select transaction_index, hash from transactions) as subquery where logs.transaction_hash = subquery.hash and logs.transaction_index <> subquery.transaction_index;"
  );

  console.log(`All logs updated!`);
}

async function wrongLogTransactionIndexCount(
  databaseUrl: string
): Promise<void> {
  const knex = getKnex(databaseUrl);

  const query = knex<DBLog>("logs")
    .whereRaw(
      "transaction_index <> (select transaction_index from transactions where hash = logs.transaction_hash)"
    )
    .count();
  const sql = query.toSQL();
  console.log("Query SQL:", sql.sql);
  const logsCount = await query;
  console.log(`Found ${logsCount[0].count} wrong logs`);
}

function getKnex(databaseUrl: string): KnexType {
  const knex = Knex({
    client: "postgresql",
    connection: {
      connectionString: databaseUrl,
      keepAlive: true,
    },
    pool: { min: 2, max: 20 },
  });
  return knex;
}
