pragma solidity >=0.6.0 <=0.8.2;

contract RecoverAccount {
    function recover(bytes32 message, bytes memory signature, bytes32 code_hash) public returns (bytes32) {
        bytes memory input = abi.encode(message, signature, code_hash);
        bytes32[1] memory output;
        assembly {
            let len := mload(input)
            if iszero(call(not(0), 0xf2, 0x0, add(input, 0x20), len, output, 288)) {
            }
        }
        return output[0];
    }

    function get() public pure returns (bytes memory) {
        bytes memory ret = hex"aabbccdd";
        return ret;
    }
}
