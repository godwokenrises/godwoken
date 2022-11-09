pragma solidity ^0.8.0;

contract EtherReceiverMock {
    bool private _acceptEther;

    event log(string txt);

    receive() external payable {
        emit log("receive");
    }

    fallback() external payable {
        emit log("fallback");
    }
}
