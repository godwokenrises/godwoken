pragma solidity >=0.4.0 <0.7.0;

interface SelfDestruct {
  function done() external;
}

contract CallerContract {
  constructor() public payable {}

  function proxyDone(address _target_addr) public {
    SelfDestruct target = SelfDestruct(_target_addr);
    target.done();
  }
}
