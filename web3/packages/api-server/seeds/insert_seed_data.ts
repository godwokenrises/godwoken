import * as Knex from "knex";
import block from "./data/block.json";
import transactions from "./data/transactions.json";
import transaction_receipts from "./data/transaction_receipts.json";
export async function seed(knex: Knex): Promise<void> {
    // Deletes ALL existing entries
    await knex("logs").del();
    await knex("transactions").del();
    await knex("blocks").del();

    const {number, hash, parentHash, logsBloom, gasLimit, gasUsed, miner, size, timestamp} = block;
    // Inserts seed entries
    await knex.transaction(async (trx) => {
    await trx("blocks").insert(
        {
            number: BigInt(number),
            hash: hash,
            parent_hash: parentHash,
            logs_bloom: logsBloom,
            gas_limit: BigInt(gasLimit),
            gas_used: BigInt(gasUsed),
            miner: miner,
            size: BigInt(size),
            timestamp: new Date(timestamp*1000),
        }
    );
    for(let i = 0; i < transactions.length; i++) {
        const tx = transactions[i];
        const tx_receipt = transaction_receipts[i];
        let returnValue = ( 
            await trx("transactions").insert({
                hash: tx.hash,
                block_number: BigInt(block.number),
                block_hash: block.hash,
                transaction_index: i,
                from_address: tx.from,
                to_address: tx.to,
                value: BigInt(tx.value),
                nonce: tx.nonce,
                gas_limit: tx.gas,
                gas_price: BigInt(tx.gasPrice),
                input: tx.input,
                v: tx.v,
                r: tx.r,
                s: tx.s,
                cumulative_gas_used: tx_receipt.cumulativeGasUsed,
                gas_used: tx_receipt.gasUsed,
                logs_bloom: tx_receipt.logsBloom,
                contract_address: tx_receipt.contractAddress,
                status: tx_receipt.status,
            }, ["id"])
        );

        const logs = tx_receipt.logs;
        let newLogs = []
        for (let j = 0; j < logs.length; j ++) {
            const newLog = {
                transaction_id: returnValue[0].id,
                transaction_hash: tx.hash,
                transaction_index: i,
                block_number: block.number,
                block_hash: block.hash,
                address: logs[j].address,
                data: logs[j].data,
                log_index: j,
                topics: logs[j].topics,
            };
            newLogs.push(newLog);
        }
        await trx("logs").insert(newLogs);
    }
}).then(function(_resp) {
    console.log("Init db with seed data complete")
}).catch(function(err) {
    console.log(err);
})
};
