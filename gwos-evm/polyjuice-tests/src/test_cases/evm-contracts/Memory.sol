pragma solidity ^0.8.6;

// The Solidity Smart Contract can use any amount of memory during the execution
// but once the execution stops, the Memory is completely wiped off for the next execution.
// Whereas Storage on the other hand is persistent, each execution of
// the Smart contract has access to the data previously stored on the storage area.
contract Memory {
  function newMemory(uint64 byteNum) pure public returns (uint256) {
    uint8[] memory m = new uint8[](byteNum);
    // for (uint64 i = 0; i < byteNum; i++) {
    //   m[i] = 1;
    // }
    return m.length;
  }
}
