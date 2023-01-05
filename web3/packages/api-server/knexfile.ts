import dotenv from "dotenv";
dotenv.config({path: "./.env"})

const knexConfig = {
  development: {
    client: "postgresql",
    connection: process.env.DATABASE_URL,
    pool: {
      min: 2,
      max: 10
    },
    migrations: {
      tableName: "knex_migrations"
    }
  }
};

export default knexConfig;
