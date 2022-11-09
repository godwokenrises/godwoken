// SPDX-License-Identifier: MIT
pragma solidity >=0.4.0 <0.7.0;

interface SimpleStorage {
  function set(uint x) external;
}

contract CallerContract {
  address public ss;
  constructor(address _ss) public payable {
    ss = _ss;
  }

  function proxySet(uint x) public {
    SimpleStorage target = SimpleStorage(ss);
    target.set(x+3);
  }
}
