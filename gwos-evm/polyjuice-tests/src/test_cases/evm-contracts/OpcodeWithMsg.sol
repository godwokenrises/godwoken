pragma solidity ^0.8.4;

contract opcodeTxWithMsg {
    opcodeTxWithMsg ptw;

    event MsgTxEvent(uint256 idx, MsgData msgData, TxData txData);
    struct MsgData {
        bytes msgData;
        bytes4 msgSig;
        uint256 msgValue;
        address msgSender;
    }

    struct TxData {
        uint256 txGasPrice;
        address txOrigin;
    }
    MsgData public msgData;
    TxData public txData;

    constructor() public payable {
        updateMsgAndTxData();
    }

    function getCurrentMsgDataAndTxData()
        public
        payable
        returns (MsgData memory, TxData memory)
    {
        return (
            MsgData(msg.data, msg.sig, msg.value, msg.sender),
            TxData(tx.gasprice, tx.origin)
        );
    }

    function getCurrentGasPrice() public payable returns (uint256) {
        (msgData, txData) = getCurrentMsgDataAndTxData();
        return txData.txGasPrice;
    }

    function updateMsgAndTxData() public payable {
        (msgData, txData) = getCurrentMsgDataAndTxData();
        emit MsgTxEvent(1, msgData, txData);
    }

    function call_updateMsgAndTxData(address _addr) public payable {
        ptw = opcodeTxWithMsg(_addr);
        ptw.updateMsgAndTxData{value: msg.value}();
    }
}
