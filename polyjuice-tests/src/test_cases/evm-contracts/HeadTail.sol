// SPDX-License-Identifier: MIT

pragma solidity ^0.8.0;

// import '@openzeppelin/contracts/utils/cryptography/ECDSA.sol';

contract HeadTail {
    address payable public userOneAddress;
    bytes public userOneSignedChoiceHash;

    address payable public userTwoAddress;
    bool public userTwoChoice;
    uint256 public userTwoChoiceSubmittedTime;

    uint256 public stake;

    event EasyLog(address _addr, uint _id);

    /**
     * @dev Returns an Ethereum Signed Message, created from a `hash`. This
     * produces hash corresponding to the one signed with the
     * https://eth.wiki/json-rpc/API#eth_sign[`eth_sign`]
     * JSON-RPC method as part of EIP-191.
     *
     * See {recover}.
     */
    function toEthSignedMessageHash(bytes32 hash) internal pure returns (bytes32) {
        // 32 is the length in bytes of hash,
        // enforced by the type signature above
        return keccak256(abi.encodePacked('\x19Ethereum Signed Message:\n32', hash));
    }

    /**
     * @dev Returns the address that signed a hashed message (`hash`) with
     * `signature`. This address can then be used for verification purposes.
     *
     * The `ecrecover` EVM opcode allows for malleable (non-unique) signatures:
     * this function rejects them by requiring the `s` value to be in the lower
     * half order, and the `v` value to be either 27 or 28.
     *
     * IMPORTANT: `hash` _must_ be the result of a hash operation for the
     * verification to be secure: it is possible to craft signatures that
     * recover to arbitrary addresses for non-hashed data. A safe way to ensure
     * this is by receiving a hash of the original message (which may otherwise
     * be too long), and then calling {toEthSignedMessageHash} on it.
     */
    function recover(bytes32 hash, bytes memory signature) internal returns (address) {
        // Check the signature length
        if (signature.length != 65) {
            revert("ECDSA: invalid signature length");
        }
        emit EasyLog(address(1), 1);

        // Divide the signature in r, s and v variables
        bytes32 r;
        bytes32 s;
        uint8 v;

        // ecrecover takes the signature parameters, and the only way to get them
        // currently is to use assembly.
        // solhint-disable-next-line no-inline-assembly
        assembly {
            r := mload(add(signature, 0x20))
            s := mload(add(signature, 0x40))
            v := byte(0, mload(add(signature, 0x60)))
        }
        emit EasyLog(address(2), 2);

        return recover(hash, v, r, s);
    }

     /**
     * @dev Overload of {ECDSA-recover} that receives the `v`,
     * `r` and `s` signature fields separately.
     */
    function recover(bytes32 hash, uint8 v, bytes32 r, bytes32 s) internal returns (address) {
        // EIP-2 still allows signature malleability for ecrecover(). Remove this possibility and make the signature
        // unique. Appendix F in the Ethereum Yellow paper (https://ethereum.github.io/yellowpaper/paper.pdf), defines
        // the valid range for s in (281): 0 < s < secp256k1n ÷ 2 + 1, and for v in (282): v ∈ {27, 28}. Most
        // signatures from current libraries generate a unique signature with an s-value in the lower half order.
        //
        // If your library generates malleable signatures, such as s-values in the upper range, calculate a new s-value
        // with 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141 - s1 and flip v from 27 to 28 or
        // vice versa. If your library also generates signatures with 0/1 for v instead 27/28, add 27 to v to accept
        // these malleable signatures as well.
        require(uint256(s) <= 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0, "ECDSA: invalid signature 's' value");
        emit EasyLog(address(3), 3);
        require(v == 27 || v == 28, "ECDSA: invalid signature 'v' value");
        emit EasyLog(address(4), 4);

        // If the signature is valid (and not malleable), return the signer address
        address signer = ecrecover(hash, v, r, s);
        emit EasyLog(signer, 5);
        require(signer != address(0), "ECDSA: invalid signature");
        emit EasyLog(signer, 6);

        return signer;
    }

    function depositUserOne(bytes memory _signedChoiceHash, uint128 _stake) public payable {
        require(
            msg.value == _stake,
            'user has to pass asset value equal to second parameter of the constructor (stake)'
        );
        require(userOneAddress == address(0), "userOneAddress can't be already set");
        stake = _stake;
        userOneAddress = payable(msg.sender);
        userOneSignedChoiceHash = _signedChoiceHash;
    }

    function depositUserTwo(bool choice) public payable {
        require(
            msg.value == stake,
            'user has to pass asset value equal to second parameter of the constructor (stake)'
        );
        require(userOneAddress != address(0), 'userOneAddress has to be already set');
        require(userTwoAddress == address(0), "userTwoAddress can't be already set");
        require(userOneAddress != msg.sender, 'userTwoAddress has to differ from userOneAddress');

        userTwoAddress = payable(msg.sender);
        userTwoChoice = choice;
        userTwoChoiceSubmittedTime = block.timestamp;
    }

    function revealUserOneChoice(bool choice, string memory secret) public returns (bool) {
        require(userOneAddress != address(0), 'userOneAddress has to be already set');
        require(
            userTwoAddress != address(0),
            'user two address has to be set before distributing prize'
        );
        // require(
        //     verify(createChoiceHash(choice, secret), userOneSignedChoiceHash) == userOneAddress,
        //     'choice signature has to be correct'
        // );
        require(address(this).balance == 2 * stake, 'prize has to be not been distributed yet');

        distributePrize(choice);

        return true;
    }

    function timeout() public returns (bool) {
        require(userOneAddress != address(0), 'userOneAddress has to be already set');
        require(
            userTwoAddress != address(0),
            'user two address has to be set before distributing prize'
        );
        require(address(this).balance == 2 * stake, 'prize has to be not been distributed yet');
        require(
            block.timestamp >= userTwoChoiceSubmittedTime + 24 hours,
            '24 hours need to pass before ability to call timeout'
        );

        userTwoAddress.transfer(2 * stake);

        return true;
    }

    function verify(bytes32 hash, bytes memory signature) public returns (address) {
        return recover(toEthSignedMessageHash(hash), signature);
    }

    function createChoiceHash(bool choice, string memory secret) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(choice, secret));
    }

    function distributePrize(bool userOneChoice) private returns (bool) {
        if (userTwoChoice == userOneChoice) {
            userTwoAddress.transfer(2 * stake);
        } else {
            userOneAddress.transfer(2 * stake);
        }

        return true;
    }
}
