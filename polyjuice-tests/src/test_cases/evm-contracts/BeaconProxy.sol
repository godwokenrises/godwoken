// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

contract DummyImplementation {
    uint256 public value;
    string public text;
    uint256[] public values;

    function initializeNonPayable() public {
        value = 10;
    }

    function initializePayable() public payable {
        value = 100;
    }

    function initializeNonPayableWithValue(uint256 _value) public {
        value = _value;
    }

    function initializePayableWithValue(uint256 _value) public payable {
        value = _value;
    }

    function initialize(
        uint256 _value,
        string memory _text,
        uint256[] memory _values
    ) public {
        value = _value;
        text = _text;
        values = _values;
    }

    function get() public pure returns (bool) {
        return true;
    }

    function version() public pure virtual returns (string memory) {
        return "V1";
    }

    function reverts() public pure {
        require(false, "DummyImplementation reverted");
    }
}

contract DummyImplementationV2 is DummyImplementation {
    function migrate(uint256 newVal) public payable {
        value = newVal;
    }

    function version() public pure override returns (string memory) {
        return "V2";
    }
}

import "@openzeppelin/contracts/proxy/beacon/UpgradeableBeacon.sol";
import "@openzeppelin/contracts/proxy/beacon/BeaconProxy.sol";

contract payableInitializationTest {
    DummyImplementation public impl1;
    DummyImplementationV2 public impl2;
    UpgradeableBeacon public ub;
    BeaconProxy public bpx;
    event GetBalance(uint256);

    function init() public {
        impl1 = new DummyImplementation();
        // impl2 = new DummyImplementationV2();
        ub = new UpgradeableBeacon(address(impl1));
    }

    function Test(bytes memory invokeSigns) public payable {
        // 0xe79f5bee0000000000000000000000000000000000000000000000000000000000000037
        bpx = new BeaconProxy{value:msg.value}(address(ub), invokeSigns);
        emit GetBalance(address(bpx).balance);
    }

    function deployBeaconProxy(bytes memory invokeSigns) public payable {
        // invokeSigns: 0xe79f5bee0000000000000000000000000000000000000000000000000000000000000037

        /* BeaconProxy constructor(address beacon, bytes data)

           Initializes the proxy with beacon.
           If data is nonempty, it’s used as data in a delegate call to the
           implementation returned by the beacon. This will typically be an
           encoded function call, and allows initializating the storage of the
           proxy like a Solidity constructor.
         */
        bpx = new BeaconProxy{value: msg.value}(address(ub), invokeSigns);

        emit GetBalance(address(bpx).balance);
    }
}
