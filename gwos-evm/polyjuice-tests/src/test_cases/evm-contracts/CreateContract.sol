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
}

contract MomContract {
  event DoLog(address indexed _from, uint _value, uint _n);
  SimpleStorage public ss;

  constructor () public payable {
    emit DoLog(msg.sender, msg.value, 3);
    ss = new SimpleStorage();
    uint value = ss.get();
    ss.set(value + 132);
  }
}
