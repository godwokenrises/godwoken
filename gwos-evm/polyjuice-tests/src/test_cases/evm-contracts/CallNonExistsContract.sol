// SPDX-License-Identifier: MIT
pragma solidity >=0.6.0 <0.9.0;

contract CallNonExistsContract {
  function rawCall(address addr) public returns (bytes memory) {
      (bool success0, ) = addr.call("");
      require(success0);
      (bool success1, bytes memory data1) = addr.call(abi.encodeWithSignature("nonExistingFunction()"));
      require(success1);
      return data1;
  }
}
