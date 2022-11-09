// SPDX-License-Identifier: MIT
pragma solidity ^0.8.7;

contract PrecompiledContracts {
    constructor() {}

    // This fun can be used to call 0x08 precompiled contract
    function callBn256PairingIstanbul() public returns (uint) {
        uint256[12] memory input;
        //(G1)x
        input[0] = uint256(0x2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da);
        //(G1)y
        input[1] = uint256(0x2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6);
        //(G2)x_1
        input[2] = uint256(0x1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc);	
        //(G2)x_0
        input[3] = uint256(0x22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9);	
        //(G2)y_1
        input[4] = uint256(0x2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90);	
        //(G2)y_0
        input[5] = uint256(0x2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e);	
        //(G1)x
        input[6] = uint256(0x0000000000000000000000000000000000000000000000000000000000000001);	
        //(G1)y
        input[7] = uint256(0x30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45);	
        //(G2)x_1
        input[8] = uint256(0x1971ff0471b09fa93caaf13cbf443c1aede09cc4328f5a62aad45f40ec133eb4);	
        //(G2)x_0
        input[9] = uint256(0x091058a3141822985733cbdddfed0fd8d6c104e9e9eff40bf5abfef9ab163bc7);	
        //(G2)y_1
        input[10] = uint256(0x2a23af9a5ce2ba2796c1f4e453a370eb0af8c212d9dc9acd8fc02c2e907baea2);	
        //(G2)y_0
        input[11] = uint256(0x23a8eb0b0996252cb548a4487da97b02422ebc0e834613f954de6c7e0afdc1fc);

        // multiplies the pairings and stores a 1 in the first element of input
        assembly {
            if iszero(call(
                not(0), // gas_limit
                0x08,   // to_address
                0,      // value
                input,
                0x0180, // input length = 32 * 12 = 384bytes
                input,  // store output over input
                0x20)   // output length is 32bytes
            ) { revert(0, 0) }
        }
        return input[0];
    }

    // The address 0x08 implements elliptic curve paring operation to perform zkSNARK verification. 
    function callBn256Pairing(bytes memory input) public returns (bytes32 result) {
        // input is a serialized bytes stream of (a1, b1, a2, b2, ..., ak, bk) from (G_1 x G_2)^k
        uint256 len = input.length;
        require(len % 192 == 0);
        assembly {
            let memPtr := mload(0x40)
            let success := call(not(0), 0x08, 0, add(input, 0x20), len, memPtr, 0x20)
            switch success
            case 0 {
                revert(0, 0)
            } default {
                result := mload(memPtr)
            }
        }
    }

    // The address 0x06 implements a native elliptic curve point addition.
    function bn256Add(bytes memory input) public returns (bytes32[2] memory result) {
        uint256 len = input.length;
        assembly {
            if iszero(call(not(0), 0x06, 0, add(input, 0x20), len, result, 0x40)) {
                // revert('0x06 precompiled contract error')
                revert(0, 0)
            }
        }
    }
    function callBn256Add(bytes32 ax, bytes32 ay, bytes32 bx, bytes32 by) public returns (bytes32[2] memory result) {
        bytes32[4] memory input;
        input[0] = ax;
        input[1] = ay;
        input[2] = bx;
        input[3] = by;
        assembly {
            let success := call(not(0), 0x06, 0, input, 0x80, result, 0x40)
            switch success
            case 0 {
                revert(0,0)
            }
        }
    }

    // The address 0x07 implements a native elliptic curve multiplication with a scalar value.
    function bn256ScalarMul(bytes memory input) public returns (bytes32[2] memory result) {
        uint256 len = input.length;
        assembly {
            if iszero(call(not(0), 0x07, 0, add(input, 0x20), len, result, 0x40)) {
                revert(0, 0)
            }
        }
    }
    function callBn256ScalarMul(bytes32 x, bytes32 y, bytes32 scalar) public returns (bytes32[2] memory result) {
        bytes32[3] memory input;
        input[0] = x;
        input[1] = y;
        input[2] = scalar;
        assembly {
            let success := call(not(0), 0x07, 0, input, 0x60, result, 0x40)
            switch success
            case 0 {
                revert(0,0)
            }
        }
    }
}
