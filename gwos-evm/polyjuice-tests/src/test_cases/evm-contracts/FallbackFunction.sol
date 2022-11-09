pragma solidity >=0.6.0 <0.8.4;

contract FallbackFunction {
  uint storedData;

  constructor() public payable {
    storedData = 123;
  }

  function set(uint x) public payable {
    storedData = x;
  }

  function get() public view returns (uint) {
    return storedData;
  }

  fallback() external {
      storedData = 999;
  }
}
