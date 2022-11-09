// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

contract AddressType {
    constructor() {}

    function getCode() public view returns (bytes memory) {
        return address(this).code;
    }

    function createMemoryArray() public pure returns (bytes memory) {
        // Create a dynamic byte array:
        bytes memory b = new bytes(32);
        for (uint i = 0; i < b.length; i++)
            b[i] = bytes1(uint8(i));
        return b;
    }
}
