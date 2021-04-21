use std::{convert::TryInto, usize};
#[derive(Default, Debug)]
pub struct PolyjuiceArgs {
    pub is_create: bool,
    pub is_static: bool,
    // pub gas_limit: u64,
    // pub gas_price: u128,
    pub value: u128,
    pub input: Option<Vec<u8>>,
}

impl PolyjuiceArgs {
    // https://github.com/nervosnetwork/godwoken-examples/blob/v0.1.0/packages/polyjuice/lib/index.js
    pub fn decode(args: &[u8]) -> anyhow::Result<Self> {
        // args[0], args[1] marks depth
        let is_create = if args[2] == 3u8 { true } else { false };
        let is_static = if args[3] == 1u8 { true } else { false };
        // args[4..20] all set to zero, args[21..36] marks value
        let value = u128::from_be_bytes(args[20..36].try_into()?);
        let input_size = u32::from_le_bytes(args[36..40].try_into()?);
        let input: Vec<u8> = args[40..(40 + input_size as usize)].to_vec();
        Ok(PolyjuiceArgs {
            is_create: is_create,
            is_static: is_static,
            value: value,
            input: Some(input),
        })
    }
    // pub fn decode(args: &[u8]) -> anyhow::Result<Self> {
    //     let is_create = if args[0] == 3u8 { true } else { false };
    //     let is_static = true;
    // let gas_limit = u64::from_le_bytes(args[2..10].try_into()?);
    // let gas_price = u128::from_le_bytes(args[10..26].try_into()?);
    // let value = u128::from_be_bytes(args[42..58].try_into()?);
    // let input_size = u32::from_le_bytes(args[58..62].try_into()?);
    // let input: Vec<u8> = args[62..(62+input_size as usize)].to_vec();
    // let value = u128::from_be_bytes(args[])
    //     Ok(PolyjuiceArgs {
    //         is_create: is_create,
    //         is_static: is_static,
    //         gas_limit: gas_limit,
    //         gas_price: gas_price,
    //         value: value,
    //         input: Some(input),
    //     })
    // }
}
