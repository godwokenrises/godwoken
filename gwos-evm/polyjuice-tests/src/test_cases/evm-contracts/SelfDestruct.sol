
pragma solidity >=0.4.0 <0.7.0;

contract SelfDestruct {
  address payable owner;
  constructor(address payable _owner) public payable {
    owner = _owner;
  }

  function done() public {
    selfdestruct(owner);
  }
}
