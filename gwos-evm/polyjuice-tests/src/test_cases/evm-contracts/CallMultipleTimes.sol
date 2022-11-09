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

  function proxySet(address _ss_other, uint x) public {
    SimpleStorage target = SimpleStorage(ss);
    SimpleStorage target_other = SimpleStorage(_ss_other);
    target.set(x + 0);
    target.set(x + 1);
    target_other.set(x + 3);
    target_other.set(x + 4);
    target.set(x + 2);
    target_other.set(x + 5);
  }
}
