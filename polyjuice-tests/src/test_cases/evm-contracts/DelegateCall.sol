pragma solidity >=0.4.0 <0.7.0;

contract DelegateCall {
  uint storedData;

  constructor() public payable {
    storedData = 123;
  }

  function set(address ss, uint x) public payable {
    storedData = x - 1;
    (bool success, bytes memory _result) = ss.delegatecall(abi.encodeWithSignature("set(uint256)", x));
    require(success);
  }

  function overwrite(address ss, uint x) public payable {
    (bool success, bytes memory _result) = ss.delegatecall(abi.encodeWithSignature("set(uint256)", x));
    storedData = x + 1;
    require(success);
  }

  function multiCall(address ss, uint x) public payable {
    (bool success1, bytes memory _result1) = ss.delegatecall(abi.encodeWithSignature("set(uint256)", x));
    require(success1);
    (bool success2, bytes memory _result2) = ss.delegatecall(abi.encodeWithSignature("set(uint256)", x + 2));
    require(success2);
  }

  function get() public view returns (uint) {
    return storedData;
  }
}
