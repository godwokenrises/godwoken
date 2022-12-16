//  Error code from JSON-RPC 2.0 spec
//  reference: http://www.jsonrpc.org/specification#error_object
export const PARSE_ERROR = -32700;
export const INVALID_REQUEST = -32600;
export const METHOD_NOT_FOUND = -32601;
export const INVALID_PARAMS = -32602;
export const INTERNAL_ERROR = -32603;

//  Ethereum Json Rpc compatible error code
//  some eth client impl ref:
//  - https://github.com/MetaMask/eth-rpc-errors/blob/main/src/error-constants.ts
//  - https://infura.io/docs/ethereum#section/Error-codes
export const HEADER_NOT_FOUND_ERROR = -32000;
export const RESOURCE_NOT_FOUND = -32001;
export const RESOURCE_UNAVAILABLE = -32002;
export const TRANSACTION_REJECTED = -32003;
export const METHOD_NOT_SUPPORT = -32004;
export const LIMIT_EXCEEDED = -32005;
export const TRANSACTION_EXECUTION_ERROR = -32000;

// Polyjuice Chain custom error
// TODO - WEB3_ERROR is pretty generalize error
// later when we have more time, we can split into more detail one
export const WEB3_ERROR = -32099;
export const GW_RPC_REQUEST_ERROR = -32098;
