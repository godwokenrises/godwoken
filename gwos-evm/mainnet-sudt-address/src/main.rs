// This script reads file from https://github.com/godwokenrises/godwoken-info/blob/9e9ad718e051e44487a4cd48022aabfff2d8cd2f/mainnet_v1/bridged-token-list.json
// And write into gwos-evm/c/mainnet_sudt_proxy.h

use std::io::Read;

fn main() {
    // read file
    let mut file = std::fs::File::open("./bridged-token-list.json").unwrap();
    let mut data = String::new();
    file.read_to_string(&mut data).unwrap();

    // parse
    let json: serde_json::Value = serde_json::from_str(&data).expect("JSON");
    let list = json.as_array().unwrap();

    // output
    let mut out = String::new();
    out.push_str(format!("#define SUDT_PROXY_ADDRS_COUNT {}\n", list.len()).as_str());
    out.push_str("uint8_t SUDT_PROXY_ADDRS[SUDT_PROXY_ADDRS_COUNT][20] = {\n");

    for token in list {
        out.push_str(
            format!(
                "/* {} - {} */\n",
                token["info"]["symbol"].as_str().unwrap(),
                token["erc20Info"]["ethAddress"]
            )
            .as_str(),
        );
        let eth_addr: Vec<_> = hex::decode(
            token["erc20Info"]["ethAddress"]
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
        )
        .unwrap();
        assert_eq!(eth_addr.len(), 20);
        out.push_str("{ ");
        for b in eth_addr {
            out.push_str(format!("{}, ", b).as_str());
        }
        out.push_str(" },\n");
    }
    out.pop(); // \n
    out.pop(); // ,
    out.push_str("\n");
    out.push_str("};");
    println!("{}", out);
}
