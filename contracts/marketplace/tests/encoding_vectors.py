"""Independent v1 Borsh/SHA-256 vectors; standard library only, no file writes.

These synthetic values are NOT deployment configuration or NFT ABI evidence.
Run from any directory: python3 contracts/marketplace/tests/encoding_vectors.py
"""

import hashlib
import pathlib
import struct


def integer(value, size):
    return value.to_bytes(size, "little")


def string(value):
    raw = value.encode("utf-8")
    return struct.pack("<I", len(raw)) + raw


def vectors():
    max64 = 2**64 - 1
    max128 = 2**128 - 1
    terms = (
        string("nft.testnet") + string("é" * 512) + string("seller.testnet")
        + integer(max64, 8) + integer(max128, 16) + integer(max64, 8)
        + integer(123, 2) + integer(1000, 2) + string("fees.testnet")
        + integer(max64, 8) + bytes([7]) * 32 + bytes([8]) * 32
    )
    for environment in (0, 1):
        domain = (
            bytes([environment]) + b"".join(bytes([n]) * 32 for n in (1, 2, 3, 4))
            + string("market.testnet") + string("nft.testnet")
        )
        descriptors = (
            b"\0" + integer(max64, 8) + terms,
            b"\1" + integer(max64 - 1, 8) + integer(max64, 8) + terms + string("buyer.testnet"),
            b"\2" + integer(max64 - 2, 8) + integer(max64 - 1, 8) + integer(max64, 8)
            + string("seller.testnet") + integer(max128, 16) + integer(max128 - 1, 16)
            + integer(1, 16) + integer(0, 16),
        )
        for kind, descriptor in enumerate(descriptors):
            budgets = (
                integer(0 if kind == 2 else 101, 8) + integer(102 if kind == 0 else 0, 8)
                + integer(103, 8) + integer(4096, 4) + bytes([5]) * 32 + bytes([6]) * 32
            )
            payload = (
                b"near-marketplace:operation:v1\0" + integer(1, 2) + domain
                + descriptor + integer(max64, 8) + budgets
            )
            yield len(payload), hashlib.sha256(payload).hexdigest()


if __name__ == "__main__":
    fixture = pathlib.Path(__file__).parent / "fixtures" / "operation_v1.sha256"
    expected = [tuple(line.split()) for line in fixture.read_text().splitlines()]
    actual = [(str(size), digest) for size, digest in vectors()]
    assert actual == expected, (actual, expected)
    print(f"{len(actual)} independent Python encoding/hash vectors passed")
