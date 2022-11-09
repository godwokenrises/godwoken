pragma solidity >=0.6.0 <=0.8.2;

contract GetChainId {
  function get() public view returns (uint256) {
    uint256 id;
    assembly {
      id := chainid()
    }
    return id;
  }
}
