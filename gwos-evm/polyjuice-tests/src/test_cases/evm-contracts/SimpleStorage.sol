pragma solidity >=0.4.0 <0.7.0;

contract SimpleStorage {
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

  receive() external payable {}
}

contract RejectedSimpleStorage {
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

}
