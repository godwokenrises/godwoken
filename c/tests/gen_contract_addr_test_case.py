#!/usr/bin/env python3
#coding: utf-8

import rlp
from sha3 import keccak_256
from binascii import hexlify


def calc_rlp(sender_hex, nonce):
    sender = bytes.fromhex(sender_hex)
    return hexlify(rlp.encode([sender, nonce])).decode('utf-8')

def calc_addr(sender_hex, nonce):
    sender = bytes.fromhex(sender_hex)
    return keccak_256(rlp.encode([sender, nonce])).hexdigest()[-40:]

if __name__ == '__main__':
    sender_hex = "004ec07d2329997267ec62b4166639513386f32e"
    nonces = [0x8e, 512, 1111, 222222, 3333333333, 4294967295] + [i for i in range(280)] ;
    for nonce in nonces:
        print('test("{}", {}, "{}", "{}");'.format(
            sender_hex,
            nonce,
            calc_rlp(sender_hex, nonce),
            calc_addr(sender_hex, nonce),
        ))
