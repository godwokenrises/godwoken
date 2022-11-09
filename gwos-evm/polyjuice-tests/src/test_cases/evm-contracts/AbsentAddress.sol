pragma solidity ^0.8.4;

contract AbsentAddress {
    constructor() {}

    function getBalance(address addr) public view returns (uint256) {
        return addr.balance;
    }

    function getCode(address addr) public view returns (bytes memory) {
        return addr.code;
    }

    function getCodeHash(address addr) public view returns (bytes32) {
        return addr.codehash;
    }

    function getCodeSize(address addr) public view returns (uint256) {
        return addr.code.length;
    }
}
