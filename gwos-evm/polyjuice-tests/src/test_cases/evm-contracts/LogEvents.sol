
pragma solidity >=0.4.0 <0.7.0;

contract LogEvents {
  event DoLog(address indexed _from, uint _value, bool _is_init);

  constructor() public payable {
    emit DoLog(msg.sender, msg.value, true);
  }

  function log() public payable {
    emit DoLog(msg.sender, msg.value, false);
  }
}
