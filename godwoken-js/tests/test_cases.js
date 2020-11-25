const depositionLockScript = {
  code_hash:
    "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
  hash_type: "type",
  args: "0xa528f2b9a51118b193178db4cf2f3db92e7df323",
};
const rollupTypeScript = {};
const depositionTransaction0 = {
  "jsonrpc": "2.0",
  "result": {
    "transaction": {
      "cell_deps": [
        {
          "dep_type": "dep_group",
          "out_point": {
            "index": "0x0",
            "tx_hash": "0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c"
          }
        }
      ],
      "hash": "0x3e0faf611735129e329088a6aeb0c1adcf86123d6e6c0063700d89e6411bb7fb",
      "header_deps": [],
      "inputs": [
        {
          "previous_output": {
            "index": "0x69",
            "tx_hash": "0xe2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c"
          },
          "since": "0x0"
        }
      ],
      "outputs": [
        {
          "capacity": "0x16b969d00",
          "lock": {
            "args": "0xb2df025a0b1bdee6a7117cbbb0543bc12e82086c",
            "code_hash": "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            "hash_type": "type"
          },
          "type": null
        },
        {
          "capacity": "0x6a94d5e3ac6130",
          "lock": {
            "args": "0x02146339de92d43813333794acfed8a8b3266a79",
            "code_hash": "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            "hash_type": "type"
          },
          "type": null
        }
      ],
      "outputs_data": [
        "0x",
        "0x"
      ],
      "version": "0x0",
      "witnesses": [
        "0x55000000100000005500000055000000410000004f881edad42c5a0b75f450decfcf6c9c25012803c7ca2065a55c1bad86ce24f34de5c03636e698901fcb48856e5502d62715a82860bc7e862fce227c15c2cf1e00"
      ]
    },
    "tx_status": {
      "block_hash": "0xdf49b9fe0e635144f181c74a4d36da231acc4810c813c7f29dd0702bf5cdfb1d",
      "status": "committed"
    }
  },
  "id": 1
};
const depositionTransaction1 = {};
const depositionTransaction2 = {};
const submitBlockTransaction = {};
const depositionRequest0 = {};
const depositionRequest1 = {};
const depositionRequest2 = {};
const depositionRequests = [
  depositionRequest0,
  depositionRequest1,
  depositionRequest2,
];

module.exports = {
    depositionLockScript,
    depositionTransaction0,
    depositionTransaction1,
    depositionTransaction2
};
