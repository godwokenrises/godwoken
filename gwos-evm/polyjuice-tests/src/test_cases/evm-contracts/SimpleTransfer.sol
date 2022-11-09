pragma solidity >=0.4.0 <0.7.0;

interface SimpleStorage {
  function set(uint x) external;
}

contract SimpleTransfer {

  constructor() public payable {}

  function transferTo(address payable target) public payable {
    target.transfer(1 wei);
  }

  /* change target contract storage first */
  function transferToSimpleStorage1(address payable _target) public payable {
    SimpleStorage target = SimpleStorage(_target);
    target.set(3);
    _target.transfer(1 wei);
  }

  /* transfer first */
  function transferToSimpleStorage2(address payable _target) public payable {
    _target.transfer(1 wei);
    SimpleStorage target = SimpleStorage(_target);
    target.set(3);
  }

  /* just transfer */
  function justTransfer(address payable to, uint amount) public payable {
    to.transfer(amount);
  }
}
